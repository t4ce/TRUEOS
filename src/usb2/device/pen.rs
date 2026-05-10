use alloc::{boxed::Box, string::String, vec::Vec as AllocVec};
use core::fmt::Write as _;
use core::future::{Future, poll_fn};
use core::task::Poll;

use crab_usb::usb_if;
use crab_usb::{DetachedTransfer, Device, EndpointBulkIn, EndpointBulkOut};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::Vec;
use spin::Mutex;

use crate::disc::block;

use super::api::claim_interface;
use super::mass::{self, MassProbeInfo, MassTarget, UasTarget};
use super::scsi::{self, SenseKey};

const MAX_MASS_RUNTIMES: usize = crate::allcaps::storage::USB_MASS_MAX_RUNTIMES;
const MAX_ACTIVE_STREAMS: usize = crate::allcaps::storage::USB_MASS_MAX_ACTIVE_STREAMS;
const MASS_BOT_KEEPALIVE_ENABLED: bool = crate::allcaps::storage::USB_MASS_BOT_KEEPALIVE_ENABLED;
const MASS_UAS_KEEPALIVE_ENABLED: bool = crate::allcaps::storage::USB_MASS_UAS_KEEPALIVE_ENABLED;
const FORCE_CONSERVATIVE_BOT: bool = crate::allcaps::storage::USB_MASS_FORCE_CONSERVATIVE_BOT;
const MASS_KEEPALIVE_MS: u64 = crate::allcaps::storage::USB_MASS_KEEPALIVE_MS;
const MASS_IO_RETRY_LIMIT: u8 = crate::allcaps::storage::USB_MASS_IO_RETRY_LIMIT;
const MASS_IO_RETRY_DELAY_MS: u64 = crate::allcaps::storage::USB_MASS_IO_RETRY_DELAY_MS;
const MASS_RUNTIME_WAIT_LIMIT: u16 = crate::allcaps::storage::USB_MASS_RUNTIME_WAIT_LIMIT;
const MASS_RUNTIME_WAIT_DELAY_MS: u64 = crate::allcaps::storage::USB_MASS_RUNTIME_WAIT_DELAY_MS;
const MIN_IO_BYTES: usize = crate::allcaps::storage::USB_MASS_MIN_IO_BYTES;
const MAX_IO_BYTES: usize = crate::allcaps::storage::USB_MASS_MAX_IO_BYTES;
const MASS_IO_GROW_SUCCESS_TARGET: u16 = crate::allcaps::storage::USB_MASS_IO_GROW_SUCCESS_TARGET;
const MASS_IO_GROW_SUCCESS_TARGET_FAST_BOT: u16 =
    crate::allcaps::storage::USB_MASS_IO_GROW_SUCCESS_TARGET_FAST_BOT;
const FAST_BOT_INITIAL_IO_BYTES: usize =
    crate::allcaps::storage::USB_MASS_FAST_BOT_INITIAL_IO_BYTES;
const FAST_BOT_WRITE_MAX_IO_BYTES: usize =
    crate::allcaps::storage::USB_MASS_FAST_BOT_WRITE_MAX_IO_BYTES;
const SKHYNIX_USE_UAS: bool = crate::allcaps::storage::USB_MASS_SKHYNIX_USE_UAS;
const USB_DT_INTERFACE: u8 = 0x04;
const USB_DT_ENDPOINT: u8 = 0x05;
const USB_DT_PIPE_USAGE: u8 = 0x24;
const USB_DT_SS_ENDPOINT_COMPANION: u8 = 0x30;
const UAS_PIPE_ID_COMMAND: u8 = 1;
const UAS_PIPE_ID_STATUS: u8 = 2;
const UAS_PIPE_ID_DATA_IN: u8 = 3;
const UAS_PIPE_ID_DATA_OUT: u8 = 4;
pub(crate) const UAS_BENCH_DEFAULT_TOTAL_BYTES: u64 = 128 * 1024 * 1024;
pub(crate) const UAS_BENCH_DEFAULT_CHUNK_BYTES: usize = 256 * 1024;
pub(crate) const UAS_BENCH_DEFAULT_MAX_INFLIGHT: usize = 8;
const UAS_BENCH_STATUS_BYTES: usize = 96;
const UAS_BENCH_TICK_MS: u64 = 1;
const UAS_BENCH_FLIGHT_TIMEOUT_MS: u64 = 5_000;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ActiveMassStream {
    controller_id: u32,
    slot_id: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum MassIoProfile {
    ConservativeBot,
    FastBot,
    UasSkhynix,
}

enum UsbMassEndpoints {
    Bot {
        bulk_in: EndpointBulkIn,
        bulk_out: EndpointBulkOut,
    },
    UasSkhynix {
        command_out: EndpointBulkOut,
        status_in: EndpointBulkIn,
        data_in: EndpointBulkIn,
        data_out: EndpointBulkOut,
    },
}

struct UsbMassRuntime {
    controller_id: u32,
    slot_id: u32,
    runtime_key: u64,
    vendor_id: u16,
    product_id: u16,
    device: Device,
    interface_number: u8,
    bulk_in_ep: u8,
    bulk_out_ep: u8,
    endpoints: UsbMassEndpoints,
    transport_kind: mass::MassTransportKind,
    io_profile: MassIoProfile,
    port_speed: usb_if::Speed,
    uas_candidate_count: u8,
    io_tag: u32,
    uas_next_stream_tag: u16,
    uas_dead_stream_mask: u32,
    uas_stream_faults: u32,
    sync_cache_unsupported: bool,
    current_max_io_bytes: usize,
    io_success_streak: u16,
}

unsafe impl Send for UsbMassRuntime {}

static ACTIVE_MASS_STREAMS: Mutex<Vec<ActiveMassStream, MAX_ACTIVE_STREAMS>> =
    Mutex::new(Vec::new());
static MASS_RUNTIMES: Mutex<Vec<UsbMassRuntime, MAX_MASS_RUNTIMES>> = Mutex::new(Vec::new());

#[derive(Copy, Clone)]
struct RegisteredMassDisk {
    runtime_key: u64,
    handle: block::DeviceHandle,
}

static REGISTERED_MASS_DISKS: Mutex<Vec<RegisteredMassDisk, MAX_MASS_RUNTIMES>> =
    Mutex::new(Vec::new());

fn register_active_mass_stream(stream: ActiveMassStream) -> bool {
    let mut streams = ACTIVE_MASS_STREAMS.lock();
    if streams.iter().any(|active| *active == stream) {
        return false;
    }
    streams.push(stream).is_ok()
}

fn unregister_active_mass_stream(stream: ActiveMassStream) {
    let mut streams = ACTIVE_MASS_STREAMS.lock();
    if let Some(idx) = streams.iter().position(|active| *active == stream) {
        streams.remove(idx);
    }
}

fn register_runtime(rt: UsbMassRuntime) {
    let mut runtimes = MASS_RUNTIMES.lock();
    if let Some(existing) = runtimes
        .iter_mut()
        .find(|existing| existing.runtime_key == rt.runtime_key)
    {
        *existing = rt;
        return;
    }
    let _ = runtimes.push(rt);
}

fn take_runtime(runtime_key: u64) -> Option<UsbMassRuntime> {
    let mut runtimes = MASS_RUNTIMES.lock();
    let idx = runtimes
        .iter()
        .position(|rt| rt.runtime_key == runtime_key)?;
    Some(runtimes.remove(idx))
}

async fn take_runtime_wait(runtime_key: u64) -> Option<UsbMassRuntime> {
    for _ in 0..=MASS_RUNTIME_WAIT_LIMIT {
        if let Some(rt) = take_runtime(runtime_key) {
            return Some(rt);
        }
        Timer::after(EmbassyDuration::from_millis(MASS_RUNTIME_WAIT_DELAY_MS)).await;
    }

    crate::log!(
        "crabusb: mass runtime wait timeout key=0x{:016X} waited_ms={}\n",
        runtime_key,
        (MASS_RUNTIME_WAIT_LIMIT as u64 + 1) * MASS_RUNTIME_WAIT_DELAY_MS
    );
    None
}

fn registered_disk(runtime_key: u64) -> Option<block::DeviceHandle> {
    let disks = REGISTERED_MASS_DISKS.lock();
    disks
        .iter()
        .find(|known| known.runtime_key == runtime_key)
        .map(|known| known.handle)
}

fn remember_registered_disk(runtime_key: u64, handle: block::DeviceHandle) {
    let mut disks = REGISTERED_MASS_DISKS.lock();
    if let Some(existing) = disks
        .iter_mut()
        .find(|existing| existing.runtime_key == runtime_key)
    {
        existing.handle = handle;
        return;
    }
    let _ = disks.push(RegisteredMassDisk {
        runtime_key,
        handle,
    });
}

#[derive(Clone, Copy)]
pub(crate) struct UasBenchConfig {
    pub total_bytes: u64,
    pub chunk_bytes: usize,
    pub max_inflight: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct UasBenchProgress {
    pub phase: &'static str,
    pub elapsed_ms: u64,
    pub interval_ms: u64,
    pub completed_bytes: u64,
    pub interval_bytes: u64,
    pub target_bytes: u64,
    pub reads_completed: u64,
    pub in_flight: usize,
    pub cwnd: usize,
    pub ssthresh: usize,
    pub chunk_bytes: usize,
    pub timeouts: u32,
    pub dead_streams: u32,
}

#[derive(Clone, Copy)]
pub(crate) struct UasBenchStats {
    pub elapsed_ms: u64,
    pub completed_bytes: u64,
    pub reads_completed: u64,
    pub final_cwnd: usize,
    pub max_inflight: usize,
    pub chunk_bytes: usize,
    pub timeouts: u32,
    pub dead_streams: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UasRoutePhase {
    Init,
    Fill,
    Submit,
    Reclaim,
    Reset,
    Done,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct UasRouteTiming {
    pub fill_ms: u64,
    pub command_ms: u64,
    pub ready_ms: u64,
    pub data_ms: u64,
    pub status_ms: u64,
    pub reclaim_ms: u64,
    pub finish_ms: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct UasRouteCounters {
    pub submitted: u64,
    pub reclaimed: u64,
    pub stalled: u64,
    pub resets: u64,
    pub quarantined: u64,
    pub bytes_submitted: u64,
    pub bytes_reclaimed: u64,
    pub max_inflight: usize,
    pub live_streams: u32,
    pub dead_streams: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UasRouteProbeKind {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct UasRouteProbeConfig {
    pub kind: UasRouteProbeKind,
    pub lba: u64,
    pub total_bytes: u64,
    pub chunk_bytes: usize,
    pub max_inflight: usize,
    pub pattern_seed: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct UasRouteProbeResult {
    pub phase: UasRoutePhase,
    pub timing: UasRouteTiming,
    pub counters: UasRouteCounters,
    pub chunk_bytes: usize,
    pub max_inflight: usize,
    pub lba: u64,
    pub total_bytes: u64,
    pub error: Option<block::Error>,
}

struct UasBenchFlight {
    tag: u16,
    lba: u32,
    blocks: u16,
    bytes: usize,
    submitted_ms: u64,
    _data: AllocVec<u8>,
    status: AllocVec<u8>,
    data_ticket: Option<DetachedTransfer>,
    status_ticket: Option<DetachedTransfer>,
    data_len: Option<usize>,
    read_ready_seen: bool,
    status_good_seen: bool,
}

enum UasBenchStep {
    Completed(UasBenchFlight),
    TimedOut(UasBenchFlight),
}

fn uas_bench_stream_in_use(flights: &AllocVec<UasBenchFlight>, tag: u16) -> bool {
    flights.iter().any(|flight| flight.tag == tag)
}

fn uas_bench_stream_disabled(disabled_stream_mask: u32, tag: u16) -> bool {
    disabled_stream_mask & (1u32 << u32::from(tag)) != 0
}

fn uas_bench_next_stream_tag(
    next_tag: &mut u16,
    flights: &AllocVec<UasBenchFlight>,
    disabled_stream_mask: u32,
) -> Option<u16> {
    for _ in 0..mass::UAS_XHCI_MAX_STREAM_ID {
        let tag = (*next_tag).clamp(1, mass::UAS_XHCI_MAX_STREAM_ID);
        *next_tag = if tag >= mass::UAS_XHCI_MAX_STREAM_ID {
            1
        } else {
            tag + 1
        };
        if !uas_bench_stream_disabled(disabled_stream_mask, tag)
            && !uas_bench_stream_in_use(flights, tag)
        {
            return Some(tag);
        }
    }
    None
}

fn uas_bench_inflight_bytes(flights: &AllocVec<UasBenchFlight>) -> u64 {
    flights
        .iter()
        .fold(0u64, |sum, flight| sum.saturating_add(flight.bytes as u64))
}

fn mass_runtime_key_for_disk(handle: block::DeviceHandle) -> Option<u64> {
    let disks = REGISTERED_MASS_DISKS.lock();
    disks
        .iter()
        .find(|known| known.handle == handle)
        .map(|known| known.runtime_key)
}

pub(crate) fn is_uas_skhynix_disk(handle: block::DeviceHandle) -> bool {
    let Some(runtime_key) = mass_runtime_key_for_disk(handle) else {
        return false;
    };
    let runtimes = MASS_RUNTIMES.lock();
    runtimes
        .iter()
        .find(|rt| rt.runtime_key == runtime_key)
        .map(|rt| matches!(rt.endpoints, UsbMassEndpoints::UasSkhynix { .. }))
        .unwrap_or(false)
}

pub(crate) fn find_uas_skhynix_route_disk() -> Option<block::DeviceHandle> {
    let runtimes = MASS_RUNTIMES.lock();
    let disks = REGISTERED_MASS_DISKS.lock();
    runtimes
        .iter()
        .filter(|rt| matches!(rt.endpoints, UsbMassEndpoints::UasSkhynix { .. }))
        .find_map(|rt| {
            disks
                .iter()
                .find(|disk| disk.runtime_key == rt.runtime_key)
                .map(|disk| disk.handle)
        })
}

fn uas_bench_now_ms() -> u64 {
    let ticks = embassy_time_driver::now();
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        0
    } else {
        ticks.saturating_mul(1000) / hz
    }
}

fn uas_bench_transfer_error(stage: &'static str) -> block::Error {
    let submit = crab_usb::debug_last_submit();
    let event = crab_usb::debug_last_event();
    crate::log!(
        "crabusb: mass uas-bench transfer-error stage={} last_submit[slot={} dci={} dir={} stream={} len={} ptr=0x{:X} ring=0x{:X}] last_event[slot={} ep={} cc={} residual={} ptr=0x{:X}]\n",
        stage,
        submit.slot_id,
        submit.dci,
        submit.direction,
        submit.stream_id,
        submit.len,
        submit.ptr,
        submit.ring_ptr,
        event.slot_id,
        event.ep_id,
        event.completion_code,
        event.residual,
        event.ptr
    );
    block::Error::Io
}

fn uas_bench_submit_status(
    status_in: &mut EndpointBulkIn,
    tag: u16,
    status: &mut AllocVec<u8>,
) -> block::Result<DetachedTransfer> {
    for byte in status.iter_mut() {
        *byte = 0;
    }
    // The flight owns this heap buffer and keeps it alive until this ticket completes.
    unsafe { status_in.submit_on_stream_detached(tag, status.as_mut_slice()) }
        .map_err(|_| uas_bench_transfer_error("status-submit"))
}

fn uas_bench_submit_data(
    data_in: &mut EndpointBulkIn,
    tag: u16,
    data: &mut AllocVec<u8>,
) -> block::Result<DetachedTransfer> {
    // The flight owns this heap buffer and keeps it alive until this ticket completes.
    unsafe { data_in.submit_on_stream_detached(tag, data.as_mut_slice()) }
        .map_err(|_| uas_bench_transfer_error("data-submit"))
}

fn uas_bench_forget_pending(flights: &mut AllocVec<UasBenchFlight>) {
    while let Some(flight) = flights.pop() {
        core::mem::forget(flight);
    }
}

async fn uas_bench_submit_read(
    command_out: &mut EndpointBulkOut,
    status_in: &mut EndpointBulkIn,
    data_in: &mut EndpointBulkIn,
    tag: u16,
    lba: u32,
    blocks: u16,
    bytes: usize,
) -> block::Result<UasBenchFlight> {
    let mut status = alloc::vec![0u8; UAS_BENCH_STATUS_BYTES];
    let mut data = alloc::vec![0u8; bytes];
    let status_ticket = uas_bench_submit_status(status_in, tag, &mut status)?;
    let data_ticket = uas_bench_submit_data(data_in, tag, &mut data)?;

    let flight = UasBenchFlight {
        tag,
        lba,
        blocks,
        bytes,
        submitted_ms: uas_bench_now_ms(),
        _data: data,
        status,
        data_ticket: Some(data_ticket),
        status_ticket: Some(status_ticket),
        data_len: None,
        read_ready_seen: false,
        status_good_seen: false,
    };

    if let Err(err) = mass::send_read10_uas_skhynix(command_out, lba, blocks, u32::from(tag)).await
    {
        core::mem::forget(flight);
        return Err(map_io_error(err));
    }

    Ok(flight)
}

fn uas_bench_poll_flights(
    status_in: &mut EndpointBulkIn,
    data_in: &mut EndpointBulkIn,
    flights: &mut AllocVec<UasBenchFlight>,
    cx: &mut core::task::Context<'_>,
) -> block::Result<Option<UasBenchStep>> {
    let now_ms = uas_bench_now_ms();
    let mut idx = 0usize;
    while idx < flights.len() {
        let mut completed = false;
        let mut timed_out = false;
        let mut step_err = None;
        {
            let flight = &mut flights[idx];
            if now_ms.saturating_sub(flight.submitted_ms) > UAS_BENCH_FLIGHT_TIMEOUT_MS {
                crate::log!(
                    "crabusb: mass uas-bench timeout tag=0x{:04X} lba={} blocks={} bytes={} age_ms={} data_pending={} status_pending={} data_len={} ready={} good={}\n",
                    flight.tag,
                    flight.lba,
                    flight.blocks,
                    flight.bytes,
                    now_ms.saturating_sub(flight.submitted_ms),
                    flight.data_ticket.is_some(),
                    flight.status_ticket.is_some(),
                    flight.data_len.unwrap_or(0),
                    flight.read_ready_seen,
                    flight.status_good_seen
                );
                timed_out = true;
            }

            if !timed_out && step_err.is_none() {
                if let Some(ticket) = flight.data_ticket {
                    match data_in.poll_detached(ticket, cx) {
                        Poll::Ready(Ok(got)) => {
                            flight.data_ticket = None;
                            if got < flight.bytes {
                                crate::log!(
                                    "crabusb: mass uas-bench short-data tag=0x{:04X} lba={} got={} need={}\n",
                                    flight.tag,
                                    flight.lba,
                                    got,
                                    flight.bytes
                                );
                                step_err = Some(block::Error::Io);
                            } else {
                                flight.data_len = Some(got);
                            }
                        }
                        Poll::Ready(Err(_)) => {
                            step_err = Some(uas_bench_transfer_error("data-complete"));
                        }
                        Poll::Pending => {}
                    }
                }
            }

            if !timed_out && step_err.is_none() {
                if let Some(ticket) = flight.status_ticket {
                    match status_in.poll_detached(ticket, cx) {
                        Poll::Ready(Ok(got)) => {
                            flight.status_ticket = None;
                            let status = &flight.status[..got.min(flight.status.len())];
                            match mass::classify_uas_read_status_iu("read-10", status, flight.tag) {
                                Ok(mass::UasReadStatusKind::ReadReady) => {
                                    flight.read_ready_seen = true;
                                }
                                Ok(mass::UasReadStatusKind::StatusGood) => {
                                    flight.status_good_seen = true;
                                }
                                Err(err) => {
                                    step_err = Some(map_io_error(err));
                                }
                            }
                        }
                        Poll::Ready(Err(_)) => {
                            step_err = Some(uas_bench_transfer_error("status-complete"));
                        }
                        Poll::Pending => {}
                    }
                }
            }

            if !timed_out
                && step_err.is_none()
                && flight.read_ready_seen
                && flight.data_len.is_some()
                && !flight.status_good_seen
                && flight.status_ticket.is_none()
            {
                match uas_bench_submit_status(status_in, flight.tag, &mut flight.status) {
                    Ok(ticket) => flight.status_ticket = Some(ticket),
                    Err(err) => step_err = Some(err),
                }
            }

            if step_err.is_none() && flight.data_len.is_some() && flight.status_good_seen {
                completed = true;
            }
        }

        if timed_out {
            let flight = flights.swap_remove(idx);
            return Ok(Some(UasBenchStep::TimedOut(flight)));
        }

        if let Some(err) = step_err {
            uas_bench_forget_pending(flights);
            return Err(err);
        }

        if completed {
            let flight = flights.swap_remove(idx);
            return Ok(Some(UasBenchStep::Completed(flight)));
        }

        idx += 1;
    }

    Ok(None)
}

async fn uas_bench_wait_one(
    status_in: &mut EndpointBulkIn,
    data_in: &mut EndpointBulkIn,
    flights: &mut AllocVec<UasBenchFlight>,
) -> block::Result<Option<UasBenchStep>> {
    let mut tick = core::pin::pin!(Timer::after(EmbassyDuration::from_millis(UAS_BENCH_TICK_MS)));
    poll_fn(|cx| {
        match uas_bench_poll_flights(status_in, data_in, flights, cx) {
            Ok(Some(step)) => return Poll::Ready(Ok(Some(step))),
            Err(err) => return Poll::Ready(Err(err)),
            Ok(None) => {}
        }
        if tick.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Ok(None));
        }
        Poll::Pending
    })
    .await
}

#[allow(clippy::too_many_arguments)]
fn uas_bench_report<F>(
    report: &mut F,
    phase: &'static str,
    start_ms: u64,
    last_report_ms: &mut u64,
    last_report_bytes: &mut u64,
    completed_bytes: u64,
    target_bytes: u64,
    reads_completed: u64,
    in_flight: usize,
    cwnd: usize,
    ssthresh: usize,
    chunk_bytes: usize,
    timeouts: u32,
    disabled_stream_mask: u32,
) where
    F: FnMut(UasBenchProgress),
{
    let now_ms = uas_bench_now_ms();
    let interval_ms = now_ms.saturating_sub(*last_report_ms);
    let interval_bytes = completed_bytes.saturating_sub(*last_report_bytes);
    report(UasBenchProgress {
        phase,
        elapsed_ms: now_ms.saturating_sub(start_ms),
        interval_ms,
        completed_bytes,
        interval_bytes,
        target_bytes,
        reads_completed,
        in_flight,
        cwnd,
        ssthresh,
        chunk_bytes,
        timeouts,
        dead_streams: disabled_stream_mask.count_ones(),
    });
    *last_report_ms = now_ms;
    *last_report_bytes = completed_bytes;
}

fn route_probe_clamp_bytes(disk: block::DeviceHandle, wanted_bytes: usize) -> block::Result<usize> {
    let info = disk.info();
    let block_size = info.block_size as usize;
    if block_size == 0 || info.block_count == 0 {
        return Err(block::Error::InvalidParam);
    }

    let max_transfer = info
        .max_transfer_bytes
        .min(MAX_IO_BYTES as u64)
        .max(info.block_size as u64) as usize;
    let wanted = wanted_bytes.clamp(block_size, max_transfer);
    Ok((wanted / block_size).max(1) * block_size)
}

fn fill_route_pattern(buf: &mut [u8], pattern_seed: u64, absolute_offset: u64) {
    let mut seed = pattern_seed
        .wrapping_add(absolute_offset)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15);
    for byte in buf.iter_mut() {
        seed ^= seed << 7;
        seed ^= seed >> 9;
        seed = seed.wrapping_mul(0xD6E8_FD93_35A5_6B19);
        *byte = (seed >> 24) as u8;
    }
}

async fn set_uas_skhynix_io_window(
    disk: block::DeviceHandle,
    chunk_bytes: usize,
    max_inflight: usize,
    stage: &'static str,
) -> block::Result<(usize, usize)> {
    let Some(runtime_key) = mass_runtime_key_for_disk(disk) else {
        return Err(block::Error::NotSupported);
    };
    let block_size = disk.block_size() as usize;
    let chunk_bytes = route_probe_clamp_bytes(disk, chunk_bytes)?;
    let max_inflight = max_inflight.clamp(1, UAS_BENCH_DEFAULT_MAX_INFLIGHT);
    let mut rt = take_runtime_wait(runtime_key)
        .await
        .ok_or(block::Error::NotReady)?;

    let result = if matches!(rt.endpoints, UsbMassEndpoints::UasSkhynix { .. }) {
        rt.current_max_io_bytes = clamp_mass_io_bytes(block_size, chunk_bytes);
        rt.io_success_streak = 0;
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-window stage={} chunk_bytes={} max_inflight={}\n",
            rt.vendor_id,
            rt.product_id,
            stage,
            rt.current_max_io_bytes,
            max_inflight
        );
        Ok((rt.current_max_io_bytes, max_inflight))
    } else {
        Err(block::Error::NotSupported)
    };

    register_runtime(rt);
    result
}

pub(crate) async fn set_uas_skhynix_route_window(
    disk: block::DeviceHandle,
    chunk_bytes: usize,
    max_inflight: usize,
) -> block::Result<(usize, usize)> {
    set_uas_skhynix_io_window(disk, chunk_bytes, max_inflight, "route-probe").await
}

pub(crate) async fn set_uas_skhynix_write_window_for_bench(
    disk: block::DeviceHandle,
    tx_cap_bytes: usize,
    max_inflight: usize,
) -> block::Result<(usize, usize)> {
    set_uas_skhynix_io_window(disk, tx_cap_bytes, max_inflight, "bench-write").await
}

async fn reset_uas_skhynix_transport(
    disk: block::DeviceHandle,
    stage: &'static str,
) -> block::Result<()> {
    let Some(runtime_key) = mass_runtime_key_for_disk(disk) else {
        return Err(block::Error::NotSupported);
    };
    let block_size = (disk.block_size() as usize).max(1);
    let mut rt = take_runtime_wait(runtime_key)
        .await
        .ok_or(block::Error::NotReady)?;

    let result = if matches!(rt.endpoints, UsbMassEndpoints::UasSkhynix { .. }) {
        rt.uas_dead_stream_mask = 0;
        rt.uas_stream_faults = 0;
        rt.uas_next_stream_tag = mass::uas_stream_id_from_tag(rt.io_tag);
        rt.current_max_io_bytes = clamp_mass_io_bytes(block_size, MAX_IO_BYTES);
        rt.io_success_streak = 0;
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-reset stage={} io_limit={}\n",
            rt.vendor_id,
            rt.product_id,
            stage,
            rt.current_max_io_bytes
        );
        Ok(())
    } else {
        Err(block::Error::NotSupported)
    };

    register_runtime(rt);
    result
}

pub(crate) async fn reset_uas_skhynix_route_transport(
    disk: block::DeviceHandle,
    stage: &'static str,
) -> block::Result<()> {
    reset_uas_skhynix_transport(disk, stage).await
}

pub(crate) async fn reset_uas_skhynix_transport_for_bench(
    disk: block::DeviceHandle,
    stage: &'static str,
) -> block::Result<()> {
    reset_uas_skhynix_transport(disk, stage).await
}

pub(crate) async fn run_uas_skhynix_route_probe(
    disk: block::DeviceHandle,
    config: UasRouteProbeConfig,
) -> block::Result<UasRouteProbeResult> {
    if mass_runtime_key_for_disk(disk).is_none() {
        return Err(block::Error::NotSupported);
    }

    let info = disk.info();
    let block_size = info.block_size as usize;
    if block_size == 0 || info.block_count == 0 {
        return Err(block::Error::InvalidParam);
    }

    let chunk_bytes = route_probe_clamp_bytes(disk, config.chunk_bytes)?;
    let max_inflight = config.max_inflight.clamp(1, UAS_BENCH_DEFAULT_MAX_INFLIGHT);
    let total_bytes = config.total_bytes.max(block_size as u64);
    let blocks_total = total_bytes.div_ceil(block_size as u64);
    if config
        .lba
        .checked_add(blocks_total)
        .map(|end| end > info.block_count)
        .unwrap_or(true)
    {
        return Err(block::Error::OutOfBounds);
    }

    let start_ms = uas_bench_now_ms();
    let mut phase = UasRoutePhase::Init;
    let mut timing = UasRouteTiming::default();
    let mut counters = UasRouteCounters {
        live_streams: max_inflight as u32,
        dead_streams: 0,
        ..UasRouteCounters::default()
    };
    let mut completed_bytes = 0u64;
    let mut cur_lba = config.lba;
    let mut buf = AllocVec::new();
    let mut error = None;

    while completed_bytes < total_bytes {
        let remaining = total_bytes.saturating_sub(completed_bytes);
        let bytes_here = core::cmp::min(chunk_bytes as u64, remaining) as usize;
        let blocks_here = (bytes_here.div_ceil(block_size)).max(1);
        let bytes_here = blocks_here * block_size;
        buf.resize(bytes_here, 0);

        let fill_start_ms = uas_bench_now_ms();
        if config.kind == UasRouteProbeKind::Write {
            fill_route_pattern(&mut buf, config.pattern_seed, completed_bytes);
        }
        timing.fill_ms = timing
            .fill_ms
            .saturating_add(uas_bench_now_ms().saturating_sub(fill_start_ms));

        let command_start_ms = uas_bench_now_ms();
        counters.submitted = counters.submitted.saturating_add(1);
        counters.bytes_submitted = counters.bytes_submitted.saturating_add(bytes_here as u64);
        counters.max_inflight = counters.max_inflight.max(1);
        let op = match config.kind {
            UasRouteProbeKind::Read => disk.read_blocks_into(cur_lba, blocks_here, &mut buf).await,
            UasRouteProbeKind::Write => disk.write_blocks(cur_lba, &buf).await,
        };
        timing.command_ms = timing
            .command_ms
            .saturating_add(uas_bench_now_ms().saturating_sub(command_start_ms));

        match op {
            Ok(()) => {
                phase = UasRoutePhase::Reclaim;
                counters.reclaimed = counters.reclaimed.saturating_add(1);
                counters.bytes_reclaimed =
                    counters.bytes_reclaimed.saturating_add(bytes_here as u64);
                completed_bytes = completed_bytes.saturating_add(bytes_here as u64);
                cur_lba = cur_lba.saturating_add(blocks_here as u64);
            }
            Err(err) => {
                phase = UasRoutePhase::Reset;
                counters.stalled = counters.stalled.saturating_add(1);
                counters.resets = counters.resets.saturating_add(1);
                error = Some(err);
                break;
            }
        }
    }

    let finish_start_ms = uas_bench_now_ms();
    if error.is_none() && config.kind == UasRouteProbeKind::Write {
        if let Err(err) = disk.flush().await {
            phase = UasRoutePhase::Reset;
            counters.resets = counters.resets.saturating_add(1);
            error = Some(err);
        }
    }
    timing.finish_ms = timing
        .finish_ms
        .saturating_add(uas_bench_now_ms().saturating_sub(finish_start_ms));

    if error.is_none() {
        phase = UasRoutePhase::Done;
    }
    timing.reclaim_ms = timing.reclaim_ms.saturating_add(
        uas_bench_now_ms()
            .saturating_sub(start_ms)
            .saturating_sub(timing.fill_ms)
            .saturating_sub(timing.command_ms)
            .saturating_sub(timing.finish_ms),
    );

    Ok(UasRouteProbeResult {
        phase,
        timing,
        counters,
        chunk_bytes,
        max_inflight,
        lba: config.lba,
        total_bytes: completed_bytes,
        error,
    })
}

pub(crate) async fn run_uas_skhynix_stream_bench<F, S>(
    disk: block::DeviceHandle,
    config: UasBenchConfig,
    should_stop: S,
    mut report: F,
) -> block::Result<UasBenchStats>
where
    F: FnMut(UasBenchProgress),
    S: Fn() -> bool,
{
    let Some(runtime_key) = mass_runtime_key_for_disk(disk) else {
        return Err(block::Error::NotSupported);
    };
    let mut rt = take_runtime_wait(runtime_key)
        .await
        .ok_or(block::Error::NotReady)?;

    let result = async {
        if !matches!(rt.endpoints, UsbMassEndpoints::UasSkhynix { .. }) {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} uas-bench rejected transport={}\n",
                rt.vendor_id,
                rt.product_id,
                mass_transport_label(rt.transport_kind)
            );
            return Err(block::Error::NotSupported);
        }

        let info = disk.info();
        let block_size = info.block_size as usize;
        if block_size == 0 || info.block_count == 0 {
            return Err(block::Error::InvalidParam);
        }
        let max_lba_blocks = core::cmp::min(info.block_count, u32::MAX as u64);
        if max_lba_blocks == 0 {
            return Err(block::Error::OutOfBounds);
        }

        let max_read_bytes = core::cmp::min(MAX_IO_BYTES, u16::MAX as usize * block_size);
        let wanted_chunk = config.chunk_bytes.clamp(block_size, max_read_bytes);
        let chunk_bytes = clamp_mass_io_bytes(block_size, wanted_chunk);
        let max_inflight = config.max_inflight.clamp(1, UAS_BENCH_DEFAULT_MAX_INFLIGHT);
        let target_bytes = config.total_bytes.max(chunk_bytes as u64);
        let blocks_per_read = (chunk_bytes / block_size).max(1).min(u16::MAX as usize) as u16;

        let start_ms = uas_bench_now_ms();
        let mut last_report_ms = start_ms;
        let mut last_report_bytes = 0u64;
        let mut completed_bytes = 0u64;
        let mut submitted_bytes = 0u64;
        let mut reads_completed = 0u64;
        let mut timeouts = 0u32;
        let mut disabled_stream_mask = 0u32;
        let mut cwnd = 1usize;
        let mut ssthresh = max_inflight;
        let mut additive_acks = 0usize;
        let mut phase = "slow-start";
        let mut next_lba = 0u64;
        let mut next_tag = mass::uas_stream_id_from_tag(rt.io_tag);
        let mut flights: AllocVec<UasBenchFlight> = AllocVec::new();
        let mut stop_requested = false;

        uas_bench_report(
            &mut report,
            "start",
            start_ms,
            &mut last_report_ms,
            &mut last_report_bytes,
            completed_bytes,
            target_bytes,
            reads_completed,
            flights.len(),
            cwnd,
            ssthresh,
            chunk_bytes,
            timeouts,
            disabled_stream_mask,
        );

        while completed_bytes < target_bytes || !flights.is_empty() {
            if should_stop() {
                stop_requested = true;
            }

            while !stop_requested && flights.len() < cwnd && submitted_bytes < target_bytes {
                let remaining_bytes = target_bytes.saturating_sub(submitted_bytes);
                let wanted_bytes = core::cmp::min(chunk_bytes as u64, remaining_bytes) as usize;
                let mut blocks = (wanted_bytes / block_size)
                    .max(1)
                    .min(blocks_per_read as usize);
                if next_lba.saturating_add(blocks as u64) > max_lba_blocks {
                    next_lba = 0;
                }
                if next_lba.saturating_add(blocks as u64) > max_lba_blocks {
                    blocks = max_lba_blocks as usize;
                }
                let bytes = blocks.saturating_mul(block_size);
                if bytes == 0 {
                    return Err(block::Error::InvalidParam);
                }

                let Some(stream_tag) =
                    uas_bench_next_stream_tag(&mut next_tag, &flights, disabled_stream_mask)
                else {
                    if flights.is_empty() {
                        return Err(block::Error::Timeout);
                    }
                    break;
                };
                let flight = {
                    let endpoints = &mut rt.endpoints;
                    let UsbMassEndpoints::UasSkhynix {
                        command_out,
                        status_in,
                        data_in,
                        ..
                    } = endpoints
                    else {
                        return Err(block::Error::NotSupported);
                    };
                    match uas_bench_submit_read(
                        command_out,
                        status_in,
                        data_in,
                        stream_tag,
                        next_lba as u32,
                        blocks as u16,
                        bytes,
                    )
                    .await
                    {
                        Ok(flight) => flight,
                        Err(err) => {
                            uas_bench_forget_pending(&mut flights);
                            return Err(err);
                        }
                    }
                };
                rt.io_tag = rt.io_tag.wrapping_add(1);
                flights.push(flight);

                submitted_bytes = submitted_bytes.saturating_add(bytes as u64);
                next_lba = next_lba.saturating_add(blocks as u64);
            }

            if flights.is_empty() {
                if submitted_bytes >= target_bytes {
                    break;
                }
                continue;
            }

            let step_now = {
                let endpoints = &mut rt.endpoints;
                let UsbMassEndpoints::UasSkhynix {
                    status_in, data_in, ..
                } = endpoints
                else {
                    return Err(block::Error::NotSupported);
                };
                uas_bench_wait_one(status_in, data_in, &mut flights).await?
            };

            if let Some(step) = step_now {
                match step {
                    UasBenchStep::Completed(flight) => {
                        completed_bytes = completed_bytes.saturating_add(flight.bytes as u64);
                        reads_completed = reads_completed.saturating_add(1);
                        if cwnd < ssthresh {
                            cwnd = core::cmp::min(max_inflight, cwnd.saturating_mul(2).max(1));
                            phase = "slow-start";
                        } else if cwnd < max_inflight {
                            additive_acks = additive_acks.saturating_add(1);
                            if additive_acks >= cwnd {
                                cwnd += 1;
                                additive_acks = 0;
                            }
                            phase = "avoidance";
                        } else {
                            phase = "steady";
                        }
                    }
                    UasBenchStep::TimedOut(flight) => {
                        let retry_lba = flight.lba;
                        let lost_tag = flight.tag;
                        core::mem::forget(flight);

                        timeouts = timeouts.saturating_add(1);
                        disabled_stream_mask |= 1u32 << u32::from(lost_tag);
                        if disabled_stream_mask.count_ones()
                            >= u32::from(mass::UAS_XHCI_MAX_STREAM_ID)
                        {
                            return Err(block::Error::Timeout);
                        }

                        ssthresh = core::cmp::max(1, cwnd / 2);
                        cwnd = 1;
                        additive_acks = 0;
                        phase = "timeout";
                        next_lba = u64::from(retry_lba);
                        submitted_bytes =
                            completed_bytes.saturating_add(uas_bench_inflight_bytes(&flights));
                        crate::log!(
                            "crabusb: mass uas-bench backoff lost_tag=0x{:04X} retry_lba={} cwnd={} ssthresh={} dead_streams={} timeouts={}\n",
                            lost_tag,
                            retry_lba,
                            cwnd,
                            ssthresh,
                            disabled_stream_mask.count_ones(),
                            timeouts
                        );
                    }
                }
            }

            let now_ms = uas_bench_now_ms();
            if now_ms.saturating_sub(last_report_ms) >= 1000 || completed_bytes >= target_bytes {
                uas_bench_report(
                    &mut report,
                    phase,
                    start_ms,
                    &mut last_report_ms,
                    &mut last_report_bytes,
                    completed_bytes,
                    target_bytes,
                    reads_completed,
                    flights.len(),
                    cwnd,
                    ssthresh,
                    chunk_bytes,
                    timeouts,
                    disabled_stream_mask,
                );
            }
        }

        if stop_requested {
            ssthresh = core::cmp::max(1, cwnd / 2);
            cwnd = 1;
            uas_bench_report(
                &mut report,
                "stopped",
                start_ms,
                &mut last_report_ms,
                &mut last_report_bytes,
                completed_bytes,
                target_bytes,
                reads_completed,
                flights.len(),
                cwnd,
                ssthresh,
                chunk_bytes,
                timeouts,
                disabled_stream_mask,
            );
        }

        Ok(UasBenchStats {
            elapsed_ms: uas_bench_now_ms().saturating_sub(start_ms),
            completed_bytes,
            reads_completed,
            final_cwnd: cwnd,
            max_inflight,
            chunk_bytes,
            timeouts,
            dead_streams: disabled_stream_mask.count_ones(),
        })
    }
    .await;

    register_runtime(rt);
    result
}

#[derive(Clone)]
struct MassIdentity {
    runtime_key: u64,
    serial: Option<String>,
    key_kind: &'static str,
}

#[derive(Copy, Clone)]
struct UasEndpointRole {
    address: u8,
    max_packet_size: u16,
}

#[derive(Copy, Clone, Default)]
struct UasPipeRoles {
    command: Option<UasEndpointRole>,
    status: Option<UasEndpointRole>,
    data_in: Option<UasEndpointRole>,
    data_out: Option<UasEndpointRole>,
}

fn hex_descriptor(bytes: &[u8]) -> String {
    let mut out = String::new();
    for (idx, byte) in bytes.iter().enumerate() {
        if idx != 0 {
            out.push(' ');
        }
        let _ = write!(out, "{:02X}", byte);
    }
    out
}

fn endpoint_role_text(ep: Option<UasEndpointRole>) -> String {
    match ep {
        Some(ep) => alloc::format!("0x{:02X}/{}", ep.address, ep.max_packet_size),
        None => String::from("none"),
    }
}

fn uas_pipe_label(pipe_id: u8) -> &'static str {
    match pipe_id {
        UAS_PIPE_ID_COMMAND => "command",
        UAS_PIPE_ID_STATUS => "status",
        UAS_PIPE_ID_DATA_IN => "data-in",
        UAS_PIPE_ID_DATA_OUT => "data-out",
        _ => "unknown",
    }
}

fn assign_uas_pipe_role(roles: &mut UasPipeRoles, pipe_id: u8, ep: UasEndpointRole) {
    match pipe_id {
        UAS_PIPE_ID_COMMAND => roles.command = Some(ep),
        UAS_PIPE_ID_STATUS => roles.status = Some(ep),
        UAS_PIPE_ID_DATA_IN => roles.data_in = Some(ep),
        UAS_PIPE_ID_DATA_OUT => roles.data_out = Some(ep),
        _ => {}
    }
}

fn uas_target_from_roles(target: UasTarget, roles: UasPipeRoles) -> Option<UasTarget> {
    let command = roles.command?;
    let status = roles.status?;
    let data_in = roles.data_in?;
    let data_out = roles.data_out?;

    Some(UasTarget {
        configuration_value: target.configuration_value,
        interface_number: target.interface_number,
        alternate_setting: target.alternate_setting,
        command_out: command.address,
        status_in: status.address,
        data_in: data_in.address,
        data_out: data_out.address,
        command_out_max_packet_size: command.max_packet_size,
        status_in_max_packet_size: status.max_packet_size,
        data_in_max_packet_size: data_in.max_packet_size,
        data_out_max_packet_size: data_out.max_packet_size,
    })
}

async fn read_raw_config_descriptor(
    device: &mut Device,
    configuration_value: u8,
    vendor_id: u16,
    product_id: u16,
) -> Option<AllocVec<u8>> {
    let config_index = device
        .configurations()
        .iter()
        .position(|cfg| cfg.configuration_value == configuration_value)?
        as u8;

    let setup = usb_if::host::ControlSetup {
        request_type: usb_if::transfer::RequestType::Standard,
        recipient: usb_if::transfer::Recipient::Device,
        request: usb_if::transfer::Request::GetDescriptor,
        value: (u16::from(usb_if::descriptor::DescriptorType::CONFIGURATION.0) << 8)
            | u16::from(config_index),
        index: 0,
    };

    let mut header = alloc::vec![0u8; 9];
    let Ok(got) = device.control_in(setup.clone(), &mut header).await else {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-desc raw-config header failed cfg={}\n",
            vendor_id,
            product_id,
            configuration_value
        );
        return None;
    };
    if got < header.len() {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-desc raw-config header short got={} need={}\n",
            vendor_id,
            product_id,
            got,
            header.len()
        );
        return None;
    }

    let total_len = u16::from_le_bytes([header[2], header[3]]) as usize;
    if !(header.len()..=4096).contains(&total_len) {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-desc raw-config invalid total_len={}\n",
            vendor_id,
            product_id,
            total_len
        );
        return None;
    }

    let mut raw = alloc::vec![0u8; total_len];
    let Ok(got) = device.control_in(setup, raw.as_mut_slice()).await else {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-desc raw-config read failed cfg={} total_len={}\n",
            vendor_id,
            product_id,
            configuration_value,
            total_len
        );
        return None;
    };
    raw.truncate(got.min(total_len));
    if raw.len() < header.len() {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-desc raw-config short got={} need={}\n",
            vendor_id,
            product_id,
            raw.len(),
            header.len()
        );
        return None;
    }

    Some(raw)
}

fn parse_and_log_uas_descriptors(
    raw: &[u8],
    target: UasTarget,
    vendor_id: u16,
    product_id: u16,
) -> Option<UasTarget> {
    let verbose = crate::logflag::USB_MASS_UAS_ADVANCED_PROBE_LOGS;
    let mut roles = UasPipeRoles::default();
    let mut active = false;
    let mut current_ep = None;
    let mut offset = 0usize;

    while offset + 2 <= raw.len() {
        let len = raw[offset] as usize;
        let desc_type = raw[offset + 1];
        if len < 2 || offset + len > raw.len() {
            if verbose {
                crate::log!(
                    "crabusb: mass {:04X}:{:04X} uas-desc malformed off={} len={} remaining={}\n",
                    vendor_id,
                    product_id,
                    offset,
                    len,
                    raw.len().saturating_sub(offset)
                );
            }
            break;
        }

        let desc = &raw[offset..offset + len];
        match desc_type {
            USB_DT_INTERFACE if len >= 9 => {
                let interface_number = desc[2];
                let alternate_setting = desc[3];
                active = interface_number == target.interface_number
                    && alternate_setting == target.alternate_setting;
                current_ep = None;
                if active && verbose {
                    crate::log!(
                        "crabusb: mass {:04X}:{:04X} uas-desc interface off={} if#{} alt={} eps={} class={:02X}/{:02X}/{:02X} bytes=[{}]\n",
                        vendor_id,
                        product_id,
                        offset,
                        interface_number,
                        alternate_setting,
                        desc[4],
                        desc[5],
                        desc[6],
                        desc[7],
                        hex_descriptor(desc)
                    );
                }
            }
            USB_DT_ENDPOINT if active && len >= 7 => {
                let address = desc[2];
                let attributes = desc[3];
                let max_packet_size = u16::from_le_bytes([desc[4], desc[5]]) & 0x07FF;
                current_ep = Some(UasEndpointRole {
                    address,
                    max_packet_size,
                });
                if verbose {
                    crate::log!(
                        "crabusb: mass {:04X}:{:04X} uas-desc endpoint off={} ep=0x{:02X} attr=0x{:02X} mps={} intv={} bytes=[{}]\n",
                        vendor_id,
                        product_id,
                        offset,
                        address,
                        attributes,
                        max_packet_size,
                        desc[6],
                        hex_descriptor(desc)
                    );
                }
            }
            USB_DT_SS_ENDPOINT_COMPANION if active => {
                if verbose {
                    let max_burst = desc.get(2).copied().unwrap_or(0);
                    let attrs = desc.get(3).copied().unwrap_or(0);
                    let bytes_per_interval = if desc.len() >= 6 {
                        u16::from_le_bytes([desc[4], desc[5]])
                    } else {
                        0
                    };
                    crate::log!(
                        "crabusb: mass {:04X}:{:04X} uas-desc ss-companion off={} ep={} max_burst={} attr=0x{:02X} bytes_per_interval={} bytes=[{}]\n",
                        vendor_id,
                        product_id,
                        offset,
                        endpoint_role_text(current_ep),
                        max_burst,
                        attrs,
                        bytes_per_interval,
                        hex_descriptor(desc)
                    );
                }
            }
            USB_DT_PIPE_USAGE if active => {
                let pipe_id = desc.get(2).copied().unwrap_or(0);
                if let Some(ep) = current_ep {
                    assign_uas_pipe_role(&mut roles, pipe_id, ep);
                }
                if verbose {
                    crate::log!(
                        "crabusb: mass {:04X}:{:04X} uas-desc pipe-usage off={} ep={} pipe_id={} role={} bytes=[{}]\n",
                        vendor_id,
                        product_id,
                        offset,
                        endpoint_role_text(current_ep),
                        pipe_id,
                        uas_pipe_label(pipe_id),
                        hex_descriptor(desc)
                    );
                }
            }
            _ if active && verbose => {
                crate::log!(
                    "crabusb: mass {:04X}:{:04X} uas-desc extra off={} type=0x{:02X} len={} ep={} bytes=[{}]\n",
                    vendor_id,
                    product_id,
                    offset,
                    desc_type,
                    len,
                    endpoint_role_text(current_ep),
                    hex_descriptor(desc)
                );
            }
            _ => {}
        }

        offset += len;
    }

    if verbose {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-desc roles command={} status={} data_in={} data_out={}\n",
            vendor_id,
            product_id,
            endpoint_role_text(roles.command),
            endpoint_role_text(roles.status),
            endpoint_role_text(roles.data_in),
            endpoint_role_text(roles.data_out)
        );
    }

    uas_target_from_roles(target, roles)
}

async fn refine_uas_target_from_raw_descriptors(
    device: &mut Device,
    target: UasTarget,
    vendor_id: u16,
    product_id: u16,
) -> UasTarget {
    let Some(raw) =
        read_raw_config_descriptor(device, target.configuration_value, vendor_id, product_id).await
    else {
        return target;
    };

    let Some(refined) = parse_and_log_uas_descriptors(&raw, target, vendor_id, product_id) else {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-desc pipe roles incomplete; keeping descriptor-order target cmd=0x{:02X} status=0x{:02X} data_in=0x{:02X} data_out=0x{:02X}\n",
            vendor_id,
            product_id,
            target.command_out,
            target.status_in,
            target.data_in,
            target.data_out
        );
        return target;
    };

    if refined.command_out != target.command_out
        || refined.status_in != target.status_in
        || refined.data_in != target.data_in
        || refined.data_out != target.data_out
    {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-desc pipe roles refine cmd 0x{:02X}->0x{:02X} status 0x{:02X}->0x{:02X} data_in 0x{:02X}->0x{:02X} data_out 0x{:02X}->0x{:02X}\n",
            vendor_id,
            product_id,
            target.command_out,
            refined.command_out,
            target.status_in,
            refined.status_in,
            target.data_in,
            refined.data_in,
            target.data_out,
            refined.data_out
        );
    }

    refined
}

fn hash_mix(mut state: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        state ^= u64::from(b);
        state = state.wrapping_mul(0x1000_0000_01B3);
    }
    state
}

fn hash_mix_u16(state: u64, value: u16) -> u64 {
    hash_mix(state, &value.to_le_bytes())
}

fn hash_mix_u32(state: u64, value: u32) -> u64 {
    hash_mix(state, &value.to_le_bytes())
}

async fn build_mass_identity(
    device: &mut Device,
    controller_id: u32,
    slot_id: u32,
) -> MassIdentity {
    let (vendor_id, product_id, serial_index) = {
        let desc = device.descriptor();
        (desc.vendor_id, desc.product_id, desc.serial_number_string_index)
    };
    let serial = super::descriptor::read_optional_string_descriptor(device, serial_index).await;

    if let Some(serial) = serial {
        let mut key = 0xcbf2_9ce4_8422_2325u64;
        key = hash_mix(key, b"usb-mass-serial");
        key = hash_mix_u16(key, vendor_id);
        key = hash_mix_u16(key, product_id);
        key = hash_mix(key, serial.as_bytes());
        return MassIdentity {
            runtime_key: key,
            serial: Some(serial),
            key_kind: "serial",
        };
    }

    let mut key = 0xcbf2_9ce4_8422_2325u64;
    key = hash_mix(key, b"usb-mass-slot");
    key = hash_mix_u32(key, controller_id);
    key = hash_mix_u32(key, slot_id);
    MassIdentity {
        runtime_key: key,
        serial: None,
        key_kind: "slot",
    }
}

#[inline]
fn sense_is_transient(key: SenseKey) -> bool {
    matches!(key, SenseKey::NotReady | SenseKey::UnitAttention | SenseKey::AbortedCommand)
}

fn map_io_error(err: mass::MassProbeError) -> block::Error {
    match err {
        mass::MassProbeError::Transport(_) => block::Error::Io,
        mass::MassProbeError::ShortData { .. } => block::Error::Corrupted,
        mass::MassProbeError::Csw { .. } => block::Error::Io,
    }
}

fn clamp_mass_io_bytes(block_size: usize, bytes: usize) -> usize {
    let bs = block_size.max(1);
    let floor = core::cmp::max(bs, MIN_IO_BYTES.div_ceil(bs) * bs);
    let ceil = core::cmp::max(floor, (MAX_IO_BYTES / bs).max(1) * bs);
    let rounded = core::cmp::max(bs, (bytes / bs).max(1) * bs);
    rounded.clamp(floor, ceil)
}

fn mass_transport_label(kind: mass::MassTransportKind) -> &'static str {
    match kind {
        mass::MassTransportKind::Bot => "bot",
        mass::MassTransportKind::Uas => "uas",
    }
}

fn mass_io_profile_label(profile: MassIoProfile) -> &'static str {
    match profile {
        MassIoProfile::ConservativeBot => "bot-safe",
        MassIoProfile::FastBot => "bot-fast",
        MassIoProfile::UasSkhynix => "uas-skhynix",
    }
}

fn is_skhynix_pssd_x31(vendor_id: u16, product_id: u16) -> bool {
    vendor_id == 0x152E && product_id == 0x7001
}

fn is_superspeed_bulk_bot(port_speed: usb_if::Speed, target: &MassTarget) -> bool {
    matches!(port_speed, usb_if::Speed::SuperSpeed | usb_if::Speed::SuperSpeedPlus)
        && target.bulk_in_max_packet_size >= 1024
        && target.bulk_out_max_packet_size >= 1024
}

fn choose_mass_io_profile(
    transport_kind: mass::MassTransportKind,
    vendor_id: u16,
    product_id: u16,
    port_speed: usb_if::Speed,
    target: &MassTarget,
) -> MassIoProfile {
    let fast_bot_capable = transport_kind == mass::MassTransportKind::Bot
        && is_superspeed_bulk_bot(port_speed, target);

    if fast_bot_capable && is_skhynix_pssd_x31(vendor_id, product_id) {
        return MassIoProfile::FastBot;
    }

    if transport_kind == mass::MassTransportKind::Bot && FORCE_CONSERVATIVE_BOT {
        return MassIoProfile::ConservativeBot;
    }

    if fast_bot_capable {
        MassIoProfile::FastBot
    } else {
        MassIoProfile::ConservativeBot
    }
}

fn initial_mass_io_bytes(
    profile: MassIoProfile,
    port_speed: usb_if::Speed,
    target: &MassTarget,
    block_size: usize,
) -> usize {
    let wanted = match profile {
        MassIoProfile::ConservativeBot => MIN_IO_BYTES,
        MassIoProfile::UasSkhynix => MAX_IO_BYTES,
        MassIoProfile::FastBot => {
            if matches!(port_speed, usb_if::Speed::SuperSpeedPlus)
                || (target.bulk_in_max_packet_size >= 1024
                    && target.bulk_out_max_packet_size >= 1024)
            {
                MAX_IO_BYTES
            } else {
                FAST_BOT_INITIAL_IO_BYTES
            }
        }
    };
    clamp_mass_io_bytes(block_size, wanted)
}

fn mass_io_grow_success_target(rt: &UsbMassRuntime) -> u16 {
    match rt.io_profile {
        MassIoProfile::ConservativeBot => MASS_IO_GROW_SUCCESS_TARGET,
        MassIoProfile::FastBot => MASS_IO_GROW_SUCCESS_TARGET_FAST_BOT,
        MassIoProfile::UasSkhynix => MASS_IO_GROW_SUCCESS_TARGET_FAST_BOT,
    }
}

fn current_mass_io_bytes(rt: &UsbMassRuntime, block_size: usize) -> usize {
    clamp_mass_io_bytes(block_size, rt.current_max_io_bytes)
}

fn current_mass_write_io_bytes(rt: &UsbMassRuntime, block_size: usize) -> usize {
    let cur = current_mass_io_bytes(rt, block_size);
    match rt.io_profile {
        MassIoProfile::ConservativeBot => cur,
        MassIoProfile::UasSkhynix => cur,
        MassIoProfile::FastBot => {
            core::cmp::min(cur, clamp_mass_io_bytes(block_size, FAST_BOT_WRITE_MAX_IO_BYTES))
        }
    }
}

fn mass_io_note_success(rt: &mut UsbMassRuntime, block_size: usize) {
    rt.io_success_streak = rt.io_success_streak.saturating_add(1);
    let cur = current_mass_io_bytes(rt, block_size);
    if rt.io_success_streak >= mass_io_grow_success_target(rt) && cur < MAX_IO_BYTES {
        rt.current_max_io_bytes = clamp_mass_io_bytes(block_size, cur.saturating_mul(2));
        rt.io_success_streak = 0;
    }
}

fn mass_io_backoff(rt: &mut UsbMassRuntime, block_size: usize) {
    let cur = current_mass_io_bytes(rt, block_size);
    rt.current_max_io_bytes = clamp_mass_io_bytes(block_size, core::cmp::max(block_size, cur / 2));
    rt.io_success_streak = 0;
}

fn uas_stream_mask(tag: u16) -> u32 {
    if tag == 0 || tag > mass::UAS_XHCI_MAX_STREAM_ID {
        0
    } else {
        1u32 << u32::from(tag)
    }
}

fn uas_runtime_alloc_stream(rt: &mut UsbMassRuntime) -> Option<u16> {
    for _ in 0..mass::UAS_XHCI_MAX_STREAM_ID {
        let tag = rt
            .uas_next_stream_tag
            .clamp(1, mass::UAS_XHCI_MAX_STREAM_ID);
        rt.uas_next_stream_tag = if tag >= mass::UAS_XHCI_MAX_STREAM_ID {
            1
        } else {
            tag + 1
        };

        if rt.uas_dead_stream_mask & uas_stream_mask(tag) == 0 {
            rt.io_tag = u32::from(tag);
            return Some(tag);
        }
    }
    None
}

fn uas_error_retires_stream(err: mass::MassProbeError) -> bool {
    matches!(
        err.transport_reason(),
        Some("uas-command-timeout" | "uas-status-timeout" | "uas-data-timeout" | "uas-in-timeout")
    )
}

fn uas_runtime_note_error(
    rt: &mut UsbMassRuntime,
    stage: &'static str,
    tag: u16,
    err: mass::MassProbeError,
    block_size: usize,
) {
    mass_io_backoff(rt, block_size);
    if !uas_error_retires_stream(err) {
        return;
    }

    let mask = uas_stream_mask(tag);
    if mask == 0 || rt.uas_dead_stream_mask & mask != 0 {
        return;
    }

    rt.uas_dead_stream_mask |= mask;
    rt.uas_stream_faults = rt.uas_stream_faults.saturating_add(1);
    crate::globalog::log_with_level(
        log::Level::Warn,
        format_args!(
            "crabusb: mass {:04X}:{:04X} uas-stream retire stage={} tag=0x{:04X} err={:?} dead={} faults={} io_limit={}\n",
            rt.vendor_id,
            rt.product_id,
            stage,
            tag,
            err,
            rt.uas_dead_stream_mask.count_ones(),
            rt.uas_stream_faults,
            current_mass_io_bytes(rt, block_size)
        ),
    );
}

fn uas_runtime_log_exhausted(rt: &UsbMassRuntime, stage: &'static str) {
    crate::globalog::log_with_level(
        log::Level::Warn,
        format_args!(
            "crabusb: mass {:04X}:{:04X} uas-stream exhausted stage={} dead={} faults={}\n",
            rt.vendor_id,
            rt.product_id,
            stage,
            rt.uas_dead_stream_mask.count_ones(),
            rt.uas_stream_faults
        ),
    );
}

fn log_transport_mismatch(rt: &UsbMassRuntime, stage: &'static str) {
    let submit = crab_usb::debug_last_submit();
    let event = crab_usb::debug_last_event();
    crate::log!(
        "crabusb: mass {:04X}:{:04X} transport stage={} key=0x{:X} expect[ctrl={} slot={} out_ep=0x{:02X} in_ep=0x{:02X}] last_submit[slot={} dci={} dir={} stream={} len={} ptr=0x{:X} ring=0x{:X}] last_event[slot={} ep={} cc={} residual={} ptr=0x{:X}]\n",
        rt.vendor_id,
        rt.product_id,
        stage,
        rt.runtime_key,
        rt.controller_id,
        rt.slot_id,
        rt.bulk_out_ep,
        rt.bulk_in_ep,
        submit.slot_id,
        submit.dci,
        submit.direction,
        submit.stream_id,
        submit.len,
        submit.ptr,
        submit.ring_ptr,
        event.slot_id,
        event.ep_id,
        event.completion_code,
        event.residual,
        event.ptr
    );
}

async fn recover_runtime_transport(
    rt: &mut UsbMassRuntime,
    stage: &'static str,
    err: mass::MassProbeError,
) -> Result<(), mass::MassProbeError> {
    if let Some(reason) = err.transport_reason() {
        log_transport_mismatch(rt, stage);
        crate::log!(
            "crabusb: mass {:04X}:{:04X} recovery stage={} reason={} if#{} out_ep=0x{:02X} in_ep=0x{:02X}\n",
            rt.vendor_id,
            rt.product_id,
            stage,
            reason,
            rt.interface_number,
            rt.bulk_out_ep,
            rt.bulk_in_ep
        );
        mass::bot_recovery(&mut rt.device, rt.interface_number, rt.bulk_out_ep, rt.bulk_in_ep)
            .await?;
    }
    Ok(())
}

struct UsbMassBlockDevice {
    runtime_key: u64,
    block_size: u32,
    block_count: u64,
}

impl UsbMassBlockDevice {
    async fn with_runtime<R>(
        &self,
        f: impl for<'a> FnOnce(
            &'a mut UsbMassRuntime,
        ) -> core::pin::Pin<
            Box<dyn core::future::Future<Output = block::Result<R>> + 'a>,
        >,
    ) -> block::Result<R> {
        let mut rt = take_runtime_wait(self.runtime_key)
            .await
            .ok_or(block::Error::NotReady)?;
        let result = f(&mut rt).await;
        register_runtime(rt);
        result
    }
}

impl block::BlockDevice for UsbMassBlockDevice {
    fn block_size_bytes(&self) -> u32 {
        self.block_size
    }

    fn block_count(&self) -> u64 {
        self.block_count
    }

    fn read_blocks<'a>(
        &'a mut self,
        lba: u64,
        blocks: usize,
    ) -> block::BoxFuture<'a, block::Result<AllocVec<u8>>> {
        Box::pin(async move {
            let block_size = self.block_size as usize;
            if block_size == 0 {
                return Err(block::Error::InvalidParam);
            }
            if blocks == 0 {
                return Ok(AllocVec::new());
            }
            let blocks_u64 = blocks as u64;
            let end = lba
                .checked_add(blocks_u64)
                .ok_or(block::Error::OutOfBounds)?;
            if end > self.block_count {
                return Err(block::Error::OutOfBounds);
            }

            let total_bytes = blocks
                .checked_mul(block_size)
                .ok_or(block::Error::InvalidParam)?;
            let mut out = alloc::vec![0u8; total_bytes];

            self.with_runtime(|rt| {
                Box::pin(async move {
                    let mut cur_lba = lba;
                    let mut remaining = out.as_mut_slice();
                    while !remaining.is_empty() {
                        let max_blocks = (current_mass_io_bytes(rt, block_size) / block_size).max(1);
                        let blocks_here = core::cmp::min(max_blocks, remaining.len() / block_size);
                        let bytes_here = blocks_here * block_size;

                        let mut attempts = 0u8;
                        loop {
                            let bulk_out_ep = rt.bulk_out_ep;
                            let bulk_in_ep = rt.bulk_in_ep;
                            let uas_tag = if matches!(&rt.endpoints, UsbMassEndpoints::UasSkhynix { .. }) {
                                let Some(tag) = uas_runtime_alloc_stream(rt) else {
                                    uas_runtime_log_exhausted(rt, "read-10");
                                    return Err(block::Error::Timeout);
                                };
                                tag
                            } else {
                                0
                            };
                            let command_tag = if uas_tag != 0 {
                                u32::from(uas_tag)
                            } else {
                                rt.io_tag
                            };
                            let result = match &mut rt.endpoints {
                                UsbMassEndpoints::Bot { bulk_in, bulk_out } => {
                                    mass::read_blocks_bot(
                                        bulk_out,
                                        bulk_in,
                                        bulk_out_ep,
                                        bulk_in_ep,
                                        cur_lba as u32,
                                        blocks_here as u16,
                                        &mut remaining[..bytes_here],
                                        command_tag,
                                    )
                                    .await
                                }
                                UsbMassEndpoints::UasSkhynix {
                                    command_out,
                                    status_in,
                                    data_in,
                                    ..
                                } => {
                                    mass::read_blocks_uas_skhynix(
                                        command_out,
                                        status_in,
                                        data_in,
                                        cur_lba as u32,
                                        blocks_here as u16,
                                        &mut remaining[..bytes_here],
                                        command_tag,
                                    )
                                    .await
                                }
                            };
                            if uas_tag == 0 {
                                rt.io_tag = rt.io_tag.wrapping_add(1);
                            }

                            match result {
                                Ok(()) => {
                                    mass_io_note_success(rt, block_size);
                                    break;
                                }
                                Err(err) => {
                                    let recovered = if uas_tag != 0 {
                                        uas_runtime_note_error(rt, "read-10", uas_tag, err, block_size);
                                        false
                                    } else if matches!(rt.endpoints, UsbMassEndpoints::Bot { .. }) {
                                        mass_io_backoff(rt, block_size);
                                        recover_runtime_transport(rt, "read-10", err).await.is_ok()
                                    } else {
                                        mass_io_backoff(rt, block_size);
                                        false
                                    };
                                    if (recovered || (uas_tag != 0 && uas_error_retires_stream(err)))
                                        && attempts < MASS_IO_RETRY_LIMIT
                                    {
                                        attempts = attempts.wrapping_add(1);
                                        Timer::after(EmbassyDuration::from_millis(MASS_IO_RETRY_DELAY_MS)).await;
                                        continue;
                                    }
                                    if uas_tag != 0 && uas_error_retires_stream(err) {
                                        return Err(block::Error::Timeout);
                                    }
                                    let sense = if uas_tag != 0 {
                                        let Some(sense_tag) = uas_runtime_alloc_stream(rt) else {
                                            uas_runtime_log_exhausted(rt, "request-sense");
                                            return Err(block::Error::Timeout);
                                        };
                                        match &mut rt.endpoints {
                                            UsbMassEndpoints::UasSkhynix {
                                                command_out,
                                                status_in,
                                                data_in,
                                                ..
                                            } => {
                                                match mass::request_sense_fixed_uas_skhynix_result(
                                                    command_out,
                                                    status_in,
                                                    data_in,
                                                    u32::from(sense_tag),
                                                )
                                                .await
                                                {
                                                    Ok(sense) => sense,
                                                    Err(sense_err) => {
                                                        uas_runtime_note_error(
                                                            rt,
                                                            "request-sense",
                                                            sense_tag,
                                                            sense_err,
                                                            block_size,
                                                        );
                                                        None
                                                    }
                                                }
                                            }
                                            UsbMassEndpoints::Bot { .. } => None,
                                        }
                                    } else {
                                        let sense = match &mut rt.endpoints {
                                            UsbMassEndpoints::Bot { bulk_in, bulk_out } => {
                                                mass::request_sense_fixed(
                                                    bulk_out,
                                                    bulk_in,
                                                    bulk_out_ep,
                                                    bulk_in_ep,
                                                    rt.io_tag,
                                                )
                                                .await
                                            }
                                            UsbMassEndpoints::UasSkhynix { .. } => None,
                                        };
                                        rt.io_tag = rt.io_tag.wrapping_add(1);
                                        sense
                                    };
                                    if let Some(sense) = sense
                                        && sense_is_transient(sense.sense_key)
                                        && attempts < MASS_IO_RETRY_LIMIT
                                    {
                                        attempts = attempts.wrapping_add(1);
                                        Timer::after(EmbassyDuration::from_millis(MASS_IO_RETRY_DELAY_MS)).await;
                                        continue;
                                    }
                                    crate::log!(
                                        "crabusb: mass {:04X}:{:04X} read lba={} blocks={} err={:?} sense={:?}\n",
                                        rt.vendor_id,
                                        rt.product_id,
                                        cur_lba,
                                        blocks_here,
                                        err,
                                        sense.map(|s| (s.sense_key, s.asc, s.ascq))
                                    );
                                    return Err(map_io_error(err));
                                }
                            }
                        }

                        remaining = &mut remaining[bytes_here..];
                        cur_lba = cur_lba.saturating_add(blocks_here as u64);
                    }

                    Ok(core::mem::take(&mut out))
                })
            })
            .await
        })
    }

    fn write_blocks<'a>(
        &'a mut self,
        lba: u64,
        buf: &'a [u8],
    ) -> block::BoxFuture<'a, block::Result<()>> {
        Box::pin(async move {
            let block_size = self.block_size as usize;
            if block_size == 0 || !buf.len().is_multiple_of(block_size) {
                return Err(block::Error::InvalidParam);
            }
            if buf.is_empty() {
                return Ok(());
            }
            let blocks_total = (buf.len() / block_size) as u64;
            let end = lba
                .checked_add(blocks_total)
                .ok_or(block::Error::OutOfBounds)?;
            if end > self.block_count {
                return Err(block::Error::OutOfBounds);
            }

            let mut rt = take_runtime_wait(self.runtime_key)
                .await
                .ok_or(block::Error::NotReady)?;
            let result = async {
                let mut cur_lba = lba;
                let mut remaining = buf;
                while !remaining.is_empty() {
                    let max_blocks =
                        (current_mass_write_io_bytes(&rt, block_size) / block_size).max(1);
                    let blocks_here = core::cmp::min(max_blocks, remaining.len() / block_size);
                    let bytes_here = blocks_here * block_size;

                    let mut attempts = 0u8;
                    loop {
                        let bulk_out_ep = rt.bulk_out_ep;
                        let bulk_in_ep = rt.bulk_in_ep;
                        let uas_tag =
                            if matches!(&rt.endpoints, UsbMassEndpoints::UasSkhynix { .. }) {
                                let Some(tag) = uas_runtime_alloc_stream(&mut rt) else {
                                    uas_runtime_log_exhausted(&rt, "write-10");
                                    return Err(block::Error::Timeout);
                                };
                                tag
                            } else {
                                0
                            };
                        let command_tag = if uas_tag != 0 {
                            u32::from(uas_tag)
                        } else {
                            rt.io_tag
                        };
                        let result = match &mut rt.endpoints {
                            UsbMassEndpoints::Bot { bulk_in, bulk_out } => {
                                mass::write_blocks_bot(
                                    bulk_out,
                                    bulk_in,
                                    bulk_out_ep,
                                    bulk_in_ep,
                                    cur_lba as u32,
                                    blocks_here as u16,
                                    &remaining[..bytes_here],
                                    command_tag,
                                )
                                .await
                            }
                            UsbMassEndpoints::UasSkhynix {
                                command_out,
                                status_in,
                                data_out,
                                ..
                            } => {
                                mass::write_blocks_uas_skhynix(
                                    command_out,
                                    status_in,
                                    data_out,
                                    cur_lba as u32,
                                    blocks_here as u16,
                                    &remaining[..bytes_here],
                                    command_tag,
                                )
                                .await
                            }
                        };
                        if uas_tag == 0 {
                            rt.io_tag = rt.io_tag.wrapping_add(1);
                        }

                        match result {
                            Ok(()) => {
                                mass_io_note_success(&mut rt, block_size);
                                break;
                            }
                            Err(err) => {
                                let recovered = if uas_tag != 0 {
                                    uas_runtime_note_error(
                                        &mut rt, "write-10", uas_tag, err, block_size,
                                    );
                                    false
                                } else if matches!(rt.endpoints, UsbMassEndpoints::Bot { .. }) {
                                    mass_io_backoff(&mut rt, block_size);
                                    recover_runtime_transport(&mut rt, "write-10", err).await.is_ok()
                                } else {
                                    mass_io_backoff(&mut rt, block_size);
                                    false
                                };
                                if (recovered || (uas_tag != 0 && uas_error_retires_stream(err)))
                                    && attempts < MASS_IO_RETRY_LIMIT
                                {
                                    attempts = attempts.wrapping_add(1);
                                    Timer::after(EmbassyDuration::from_millis(MASS_IO_RETRY_DELAY_MS)).await;
                                    continue;
                                }
                                if uas_tag != 0 && uas_error_retires_stream(err) {
                                    return Err(block::Error::Timeout);
                                }
                                let sense = if uas_tag != 0 {
                                    let Some(sense_tag) = uas_runtime_alloc_stream(&mut rt) else {
                                        uas_runtime_log_exhausted(&rt, "request-sense");
                                        return Err(block::Error::Timeout);
                                    };
                                    match &mut rt.endpoints {
                                        UsbMassEndpoints::UasSkhynix {
                                            command_out,
                                            status_in,
                                            data_in,
                                            ..
                                        } => {
                                            match mass::request_sense_fixed_uas_skhynix_result(
                                                command_out,
                                                status_in,
                                                data_in,
                                                u32::from(sense_tag),
                                            )
                                            .await
                                            {
                                                Ok(sense) => sense,
                                                Err(sense_err) => {
                                                    uas_runtime_note_error(
                                                        &mut rt,
                                                        "request-sense",
                                                        sense_tag,
                                                        sense_err,
                                                        block_size,
                                                    );
                                                    None
                                                }
                                            }
                                        }
                                        UsbMassEndpoints::Bot { .. } => None,
                                    }
                                } else {
                                    let sense = match &mut rt.endpoints {
                                        UsbMassEndpoints::Bot { bulk_in, bulk_out } => {
                                            mass::request_sense_fixed(
                                                bulk_out,
                                                bulk_in,
                                                bulk_out_ep,
                                                bulk_in_ep,
                                                rt.io_tag,
                                            )
                                            .await
                                        }
                                        UsbMassEndpoints::UasSkhynix { .. } => None,
                                    };
                                    rt.io_tag = rt.io_tag.wrapping_add(1);
                                    sense
                                };
                                if let Some(sense) = sense
                                    && sense_is_transient(sense.sense_key)
                                    && attempts < MASS_IO_RETRY_LIMIT
                                {
                                    attempts = attempts.wrapping_add(1);
                                    Timer::after(EmbassyDuration::from_millis(MASS_IO_RETRY_DELAY_MS)).await;
                                    continue;
                                }
                                crate::log!(
                                    "crabusb: mass {:04X}:{:04X} write lba={} blocks={} err={:?} sense={:?}\n",
                                    rt.vendor_id,
                                    rt.product_id,
                                    cur_lba,
                                    blocks_here,
                                    err,
                                    sense.map(|s| (s.sense_key, s.asc, s.ascq))
                                );
                                return Err(map_io_error(err));
                            }
                        }
                    }

                    remaining = &remaining[bytes_here..];
                    cur_lba = cur_lba.saturating_add(blocks_here as u64);
                }
                Ok(())
            }
            .await;
            register_runtime(rt);
            result
        })
    }

    fn dma_alignment_bytes(&self) -> u32 {
        1
    }

    fn max_transfer_bytes(&self) -> u64 {
        MAX_IO_BYTES as u64
    }

    fn supports_write(&self) -> bool {
        true
    }

    fn flush<'a>(&'a mut self) -> block::BoxFuture<'a, block::Result<()>> {
        Box::pin(async move {
            let block_size = (self.block_size as usize).max(1);
            self.with_runtime(|rt| {
                Box::pin(async move {
                    if rt.sync_cache_unsupported {
                        return Ok(());
                    }

                    let bulk_out_ep = rt.bulk_out_ep;
                    let bulk_in_ep = rt.bulk_in_ep;
                    let uas_tag =
                        if matches!(&rt.endpoints, UsbMassEndpoints::UasSkhynix { .. }) {
                            let Some(tag) = uas_runtime_alloc_stream(rt) else {
                                uas_runtime_log_exhausted(rt, "sync-cache-10");
                                return Err(block::Error::Timeout);
                            };
                            tag
                        } else {
                            0
                        };
                    let command_tag = if uas_tag != 0 {
                        u32::from(uas_tag)
                    } else {
                        rt.io_tag
                    };
                    let sync_result = match &mut rt.endpoints {
                        UsbMassEndpoints::Bot { bulk_in, bulk_out } => {
                            mass::synchronize_cache_bot(
                                bulk_out,
                                bulk_in,
                                bulk_out_ep,
                                bulk_in_ep,
                                command_tag,
                            )
                            .await
                        }
                        UsbMassEndpoints::UasSkhynix {
                            command_out,
                            status_in,
                            ..
                        } => {
                            mass::synchronize_cache_uas_skhynix(
                                command_out,
                                status_in,
                                command_tag,
                            )
                            .await
                        }
                    };
                    match sync_result {
                        Ok(()) => {
                            if uas_tag == 0 {
                                rt.io_tag = rt.io_tag.wrapping_add(1);
                            }
                            Ok(())
                        }
                        Err(err) => {
                            if uas_tag != 0 {
                                uas_runtime_note_error(
                                    rt,
                                    "sync-cache-10",
                                    uas_tag,
                                    err,
                                    block_size,
                                );
                            } else if matches!(rt.endpoints, UsbMassEndpoints::Bot { .. }) {
                                let _ = recover_runtime_transport(rt, "sync-cache-10", err).await;
                                rt.io_tag = rt.io_tag.wrapping_add(1);
                            }
                            if uas_tag != 0 && uas_error_retires_stream(err) {
                                return Err(block::Error::Timeout);
                            }
                            let sense = if uas_tag != 0 {
                                let Some(sense_tag) = uas_runtime_alloc_stream(rt) else {
                                    uas_runtime_log_exhausted(rt, "request-sense");
                                    return Err(block::Error::Timeout);
                                };
                                match &mut rt.endpoints {
                                    UsbMassEndpoints::UasSkhynix {
                                        command_out,
                                        status_in,
                                        data_in,
                                        ..
                                    } => {
                                        match mass::request_sense_fixed_uas_skhynix_result(
                                            command_out,
                                            status_in,
                                            data_in,
                                            u32::from(sense_tag),
                                        )
                                        .await
                                        {
                                            Ok(sense) => sense,
                                            Err(sense_err) => {
                                                uas_runtime_note_error(
                                                    rt,
                                                    "request-sense",
                                                    sense_tag,
                                                    sense_err,
                                                    block_size,
                                                );
                                                None
                                            }
                                        }
                                    }
                                    UsbMassEndpoints::Bot { .. } => None,
                                }
                            } else {
                                let sense = match &mut rt.endpoints {
                                    UsbMassEndpoints::Bot { bulk_in, bulk_out } => {
                                        mass::request_sense_fixed(
                                            bulk_out,
                                            bulk_in,
                                            bulk_out_ep,
                                            bulk_in_ep,
                                            rt.io_tag,
                                        )
                                        .await
                                    }
                                    UsbMassEndpoints::UasSkhynix { .. } => None,
                                };
                                rt.io_tag = rt.io_tag.wrapping_add(1);
                                sense
                            };
                            if let Some(sense) = sense
                                && sense.sense_key == scsi::SenseKey::IllegalRequest
                            {
                                crate::log!(
                                    "crabusb: mass {:04X}:{:04X} sync-cache unsupported asc={:#x} ascq={:#x}\n",
                                    rt.vendor_id,
                                    rt.product_id,
                                    sense.asc,
                                    sense.ascq
                                );
                                rt.sync_cache_unsupported = true;
                                return Ok(());
                            }
                            crate::log!(
                                "crabusb: mass {:04X}:{:04X} flush err={:?} sense={:?}\n",
                                rt.vendor_id,
                                rt.product_id,
                                err,
                                sense.map(|s| (s.sense_key, s.asc, s.ascq))
                            );
                            Err(map_io_error(err))
                        }
                    }
                })
            })
            .await
        })
    }
}

fn register_block_device(
    identity: &MassIdentity,
    vendor_id: u16,
    product_id: u16,
    probe: &MassProbeInfo,
) -> block::DeviceHandle {
    if let Some(handle) = registered_disk(identity.runtime_key) {
        return handle;
    }

    let label = alloc::format!("usbms-{:04X}:{:04X}", vendor_id, product_id);
    let mut desc = block::DeviceDescriptor::new(block::DeviceKind::Unknown).with_label(label);
    if let Some(serial) = identity.serial.as_deref() {
        desc = desc.with_serial(serial);
    }

    let handle = block::register_device(
        desc,
        UsbMassBlockDevice {
            runtime_key: identity.runtime_key,
            block_size: probe.block_size.max(1),
            block_count: probe.block_count.max(1),
        },
    );
    remember_registered_disk(identity.runtime_key, handle);
    handle
}

#[embassy_executor::task(pool_size = 8)]
pub async fn mass_storage_task(
    mut device: Device,
    controller_id: u32,
    target: MassTarget,
    transport_kind: mass::MassTransportKind,
    io_profile: MassIoProfile,
    port_speed: usb_if::Speed,
    uas_candidate_count: u8,
) {
    let desc = device.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    let slot = device.slot_id();
    let active_stream = ActiveMassStream {
        controller_id,
        slot_id: u32::from(slot),
    };

    if let Err(err) = device
        .ep_ctrl()
        .set_configuration(target.configuration_value)
        .await
    {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} set cfg={} failed: {:?}\n",
            vendor_id,
            product_id,
            target.configuration_value,
            err
        );
    }

    let mut interface =
        match claim_interface(&mut device, target.interface_number, target.alternate_setting).await
        {
            Ok(interface) => interface,
            Err(err) => {
                crate::log!(
                    "crabusb: mass {:04X}:{:04X} claim failed if#{} alt={}: {:?}\n",
                    vendor_id,
                    product_id,
                    target.interface_number,
                    target.alternate_setting,
                    err
                );
                unregister_active_mass_stream(active_stream);
                return;
            }
        };

    let bulk_out = match interface.endpoint_bulk_out(target.bulk_out).await {
        Ok(ep) => ep,
        Err(err) => {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} bulk_out open failed ep=0x{:02X}: {:?}\n",
                vendor_id,
                product_id,
                target.bulk_out,
                err
            );
            unregister_active_mass_stream(active_stream);
            return;
        }
    };
    let bulk_in = match interface.endpoint_bulk_in(target.bulk_in).await {
        Ok(ep) => ep,
        Err(err) => {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} bulk_in open failed ep=0x{:02X}: {:?}\n",
                vendor_id,
                product_id,
                target.bulk_in,
                err
            );
            unregister_active_mass_stream(active_stream);
            return;
        }
    };
    drop(interface);

    let mut bulk_out = bulk_out;
    let mut bulk_in = bulk_in;
    let probe = match mass::probe_mass_bot(
        &mut device,
        &mut bulk_out,
        &mut bulk_in,
        target.interface_number,
        target.bulk_out,
        target.bulk_in,
    )
    .await
    {
        Ok(info) => info,
        Err(err) => {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} probe failed: {:?}\n",
                vendor_id,
                product_id,
                err
            );
            unregister_active_mass_stream(active_stream);
            return;
        }
    };

    let identity = build_mass_identity(&mut device, controller_id, u32::from(slot)).await;
    let existing_handle = registered_disk(identity.runtime_key);
    let handle = register_block_device(&identity, vendor_id, product_id, &probe);
    let attach_mode = if existing_handle.is_some() {
        "reattached"
    } else {
        "registered"
    };
    let initial_io_bytes =
        initial_mass_io_bytes(io_profile, port_speed, &target, probe.block_size as usize);
    crate::log!(
        "crabusb: mass {:04X}:{:04X} ready slot={} if#{} alt={} cfg={} bulk_in=0x{:02X} bulk_out=0x{:02X} in_mps={} out_mps={} disk={} mode={} label={:?} serial={:?} key={} transport={} profile={} init_io={} speed={:?} uas_candidates={} bs={} blocks={} vendor='{}' product='{}'\n",
        vendor_id,
        product_id,
        slot,
        target.interface_number,
        target.alternate_setting,
        target.configuration_value,
        target.bulk_in,
        target.bulk_out,
        target.bulk_in_max_packet_size,
        target.bulk_out_max_packet_size,
        handle.id(),
        attach_mode,
        handle.info().label,
        identity.serial.as_deref(),
        identity.key_kind,
        mass_transport_label(transport_kind),
        mass_io_profile_label(io_profile),
        initial_io_bytes,
        port_speed,
        uas_candidate_count,
        probe.block_size,
        probe.block_count,
        probe.vendor,
        probe.product
    );

    register_runtime(UsbMassRuntime {
        controller_id,
        slot_id: u32::from(slot),
        runtime_key: identity.runtime_key,
        vendor_id,
        product_id,
        device,
        interface_number: target.interface_number,
        bulk_in_ep: target.bulk_in,
        bulk_out_ep: target.bulk_out,
        endpoints: UsbMassEndpoints::Bot { bulk_in, bulk_out },
        transport_kind,
        io_profile,
        port_speed,
        uas_candidate_count,
        io_tag: 0x544F_0000 | u32::from(slot),
        uas_next_stream_tag: 1,
        uas_dead_stream_mask: 0,
        uas_stream_faults: 0,
        sync_cache_unsupported: false,
        current_max_io_bytes: initial_io_bytes,
        io_success_streak: 0,
    });

    loop {
        Timer::after(EmbassyDuration::from_millis(MASS_KEEPALIVE_MS)).await;
        let Some(mut rt) = take_runtime(identity.runtime_key) else {
            continue;
        };

        let mut recovered = false;
        let bulk_out_ep = rt.bulk_out_ep;
        let bulk_in_ep = rt.bulk_in_ep;
        let keepalive = match &mut rt.endpoints {
            UsbMassEndpoints::Bot { bulk_in, bulk_out } => {
                if MASS_BOT_KEEPALIVE_ENABLED {
                    mass::keepalive_mass_bot(bulk_out, bulk_in, bulk_out_ep, bulk_in_ep, 0).await
                } else {
                    Ok(())
                }
            }
            UsbMassEndpoints::UasSkhynix {
                command_out,
                status_in,
                ..
            } => {
                if MASS_UAS_KEEPALIVE_ENABLED {
                    let result =
                        mass::keepalive_mass_uas_skhynix(command_out, status_in, rt.io_tag).await;
                    rt.io_tag = rt.io_tag.wrapping_add(1);
                    result
                } else {
                    Ok(())
                }
            }
        };
        if let Err(err) = keepalive {
            if matches!(rt.endpoints, UsbMassEndpoints::Bot { .. })
                && recover_runtime_transport(&mut rt, "test-unit-ready", err)
                    .await
                    .is_ok()
            {
                recovered = true;
            } else {
                crate::log!(
                    "crabusb: mass {:04X}:{:04X} lifecycle stop slot={} err={:?}\n",
                    vendor_id,
                    product_id,
                    slot,
                    err
                );
                break;
            }
        }

        register_runtime(rt);
        if recovered {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} keepalive recovered slot={}\n",
                vendor_id,
                product_id,
                slot
            );
        }
    }

    unregister_active_mass_stream(active_stream);
}

#[embassy_executor::task(pool_size = 2)]
pub async fn mass_storage_uas_skhynix_task(
    mut device: Device,
    controller_id: u32,
    target: UasTarget,
    port_speed: usb_if::Speed,
    uas_candidate_count: u8,
) {
    let desc = device.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    let slot = device.slot_id();
    let active_stream = ActiveMassStream {
        controller_id,
        slot_id: u32::from(slot),
    };

    if let Err(err) = device
        .ep_ctrl()
        .set_configuration(target.configuration_value)
        .await
    {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-skhynix set cfg={} failed: {:?}\n",
            vendor_id,
            product_id,
            target.configuration_value,
            err
        );
    }

    let target =
        refine_uas_target_from_raw_descriptors(&mut device, target, vendor_id, product_id).await;

    let mut interface =
        match claim_interface(&mut device, target.interface_number, target.alternate_setting).await
        {
            Ok(interface) => interface,
            Err(err) => {
                crate::log!(
                    "crabusb: mass {:04X}:{:04X} uas-skhynix claim failed if#{} alt={}: {:?}\n",
                    vendor_id,
                    product_id,
                    target.interface_number,
                    target.alternate_setting,
                    err
                );
                unregister_active_mass_stream(active_stream);
                return;
            }
        };
    if crate::logflag::USB_MASS_UAS_ADVANCED_PROBE_LOGS {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-adv claimed if#{} alt={} cfg={} cmd_out=0x{:02X} status_in=0x{:02X} data_in=0x{:02X} data_out=0x{:02X}\n",
            vendor_id,
            product_id,
            target.interface_number,
            target.alternate_setting,
            target.configuration_value,
            target.command_out,
            target.status_in,
            target.data_in,
            target.data_out
        );
    }

    let command_out = match interface.endpoint_bulk_out(target.command_out).await {
        Ok(ep) => ep,
        Err(err) => {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} uas-skhynix command_out open failed ep=0x{:02X}: {:?}\n",
                vendor_id,
                product_id,
                target.command_out,
                err
            );
            unregister_active_mass_stream(active_stream);
            return;
        }
    };
    let data_out = match interface.endpoint_bulk_out(target.data_out).await {
        Ok(ep) => ep,
        Err(err) => {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} uas-skhynix data_out open failed ep=0x{:02X}: {:?}\n",
                vendor_id,
                product_id,
                target.data_out,
                err
            );
            unregister_active_mass_stream(active_stream);
            return;
        }
    };
    let status_in = match interface.endpoint_bulk_in(target.status_in).await {
        Ok(ep) => ep,
        Err(err) => {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} uas-skhynix status_in open failed ep=0x{:02X}: {:?}\n",
                vendor_id,
                product_id,
                target.status_in,
                err
            );
            unregister_active_mass_stream(active_stream);
            return;
        }
    };
    let data_in = match interface.endpoint_bulk_in(target.data_in).await {
        Ok(ep) => ep,
        Err(err) => {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} uas-skhynix data_in open failed ep=0x{:02X}: {:?}\n",
                vendor_id,
                product_id,
                target.data_in,
                err
            );
            unregister_active_mass_stream(active_stream);
            return;
        }
    };
    if crate::logflag::USB_MASS_UAS_ADVANCED_PROBE_LOGS {
        let stream_cfg = crab_usb::debug_last_stream_config();
        crate::log!(
            "crabusb: mass {:04X}:{:04X} uas-adv endpoints-open cmd_out=0x{:02X}/{} status_in=0x{:02X}/{} data_in=0x{:02X}/{} data_out=0x{:02X}/{} last_stream_cfg[slot={} dci={} ep=0x{:02X} count={} maxp={} burst={} mps={} ctx=0x{:X} ring1=0x{:X}]\n",
            vendor_id,
            product_id,
            target.command_out,
            target.command_out_max_packet_size,
            target.status_in,
            target.status_in_max_packet_size,
            target.data_in,
            target.data_in_max_packet_size,
            target.data_out,
            target.data_out_max_packet_size,
            stream_cfg.slot_id,
            stream_cfg.dci,
            stream_cfg.ep_addr,
            stream_cfg.stream_count,
            stream_cfg.max_primary_streams,
            stream_cfg.max_burst,
            stream_cfg.max_packet_size,
            stream_cfg.ctx_ptr,
            stream_cfg.ring1_ptr
        );
    }
    drop(interface);

    let mut command_out = command_out;
    let mut status_in = status_in;
    let mut data_in = data_in;
    let mut data_out = data_out;
    let probe = match mass::exercise_mass_uas_skhynix(
        &mut command_out,
        &mut status_in,
        &mut data_in,
        &mut data_out,
    )
    .await
    {
        Ok(info) => info,
        Err(err) => {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} uas-skhynix exercise failed: {:?}\n",
                vendor_id,
                product_id,
                err
            );
            unregister_active_mass_stream(active_stream);
            return;
        }
    };

    let identity = build_mass_identity(&mut device, controller_id, u32::from(slot)).await;
    let existing_handle = registered_disk(identity.runtime_key);
    let handle = register_block_device(&identity, vendor_id, product_id, &probe);
    let attach_mode = if existing_handle.is_some() {
        "reattached"
    } else {
        "registered"
    };
    let initial_io_bytes = clamp_mass_io_bytes(probe.block_size as usize, MAX_IO_BYTES);
    crate::log!(
        "crabusb: mass {:04X}:{:04X} uas-skhynix ready slot={} if#{} alt={} cfg={} cmd_out=0x{:02X} status_in=0x{:02X} data_in=0x{:02X} data_out=0x{:02X} mps={}/{}/{}/{} disk={} mode={} label={:?} serial={:?} key={} transport={} profile={} init_io={} speed={:?} uas_candidates={} bs={} blocks={} vendor='{}' product='{}'\n",
        vendor_id,
        product_id,
        slot,
        target.interface_number,
        target.alternate_setting,
        target.configuration_value,
        target.command_out,
        target.status_in,
        target.data_in,
        target.data_out,
        target.command_out_max_packet_size,
        target.status_in_max_packet_size,
        target.data_in_max_packet_size,
        target.data_out_max_packet_size,
        handle.id(),
        attach_mode,
        handle.info().label,
        identity.serial.as_deref(),
        identity.key_kind,
        mass_transport_label(mass::MassTransportKind::Uas),
        mass_io_profile_label(MassIoProfile::UasSkhynix),
        initial_io_bytes,
        port_speed,
        uas_candidate_count,
        probe.block_size,
        probe.block_count,
        probe.vendor,
        probe.product
    );

    register_runtime(UsbMassRuntime {
        controller_id,
        slot_id: u32::from(slot),
        runtime_key: identity.runtime_key,
        vendor_id,
        product_id,
        device,
        interface_number: target.interface_number,
        bulk_in_ep: target.status_in,
        bulk_out_ep: target.command_out,
        endpoints: UsbMassEndpoints::UasSkhynix {
            command_out,
            status_in,
            data_in,
            data_out,
        },
        transport_kind: mass::MassTransportKind::Uas,
        io_profile: MassIoProfile::UasSkhynix,
        port_speed,
        uas_candidate_count,
        io_tag: 0x5541_0000 | u32::from(slot),
        uas_next_stream_tag: mass::uas_stream_id_from_tag(0x5541_0000 | u32::from(slot)),
        uas_dead_stream_mask: 0,
        uas_stream_faults: 0,
        sync_cache_unsupported: false,
        current_max_io_bytes: initial_io_bytes,
        io_success_streak: 0,
    });

    loop {
        Timer::after(EmbassyDuration::from_millis(MASS_KEEPALIVE_MS)).await;
        let Some(mut rt) = take_runtime(identity.runtime_key) else {
            continue;
        };
        if !MASS_UAS_KEEPALIVE_ENABLED {
            register_runtime(rt);
            continue;
        }

        let bulk_out_ep = rt.bulk_out_ep;
        let bulk_in_ep = rt.bulk_in_ep;
        let uas_tag = if matches!(&rt.endpoints, UsbMassEndpoints::UasSkhynix { .. }) {
            let Some(tag) = uas_runtime_alloc_stream(&mut rt) else {
                uas_runtime_log_exhausted(&rt, "test-unit-ready");
                break;
            };
            tag
        } else {
            0
        };
        let keepalive = match &mut rt.endpoints {
            UsbMassEndpoints::UasSkhynix {
                command_out,
                status_in,
                ..
            } => {
                let result =
                    mass::keepalive_mass_uas_skhynix(command_out, status_in, u32::from(uas_tag))
                        .await;
                result
            }
            UsbMassEndpoints::Bot { bulk_in, bulk_out } => {
                mass::keepalive_mass_bot(bulk_out, bulk_in, bulk_out_ep, bulk_in_ep, 0).await
            }
        };
        if let Err(err) = keepalive {
            if uas_tag != 0 {
                uas_runtime_note_error(
                    &mut rt,
                    "test-unit-ready",
                    uas_tag,
                    err,
                    probe.block_size as usize,
                );
                if uas_error_retires_stream(err)
                    && rt.uas_dead_stream_mask.count_ones()
                        < u32::from(mass::UAS_XHCI_MAX_STREAM_ID)
                {
                    register_runtime(rt);
                    continue;
                }
            }
            crate::log!(
                "crabusb: mass {:04X}:{:04X} uas-skhynix lifecycle stop slot={} err={:?}\n",
                vendor_id,
                product_id,
                slot,
                err
            );
            break;
        }

        register_runtime(rt);
    }

    unregister_active_mass_stream(active_stream);
}

pub(crate) async fn maybe_start_mass_storage(
    host: &mut crab_usb::USBHost,
    dev_info: &crab_usb::DeviceInfo,
    spawner: &Spawner,
    controller_id: u32,
) -> bool {
    let transport_plan = mass::inspect_mass_transports(dev_info.configurations());
    let desc = dev_info.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    let topology = dev_info.topology();
    let uas_candidate_count = transport_plan.uas.len().min(u8::MAX as usize) as u8;

    for uas in transport_plan.uas.iter() {
        let mut bulk_in = String::new();
        for (idx, ep) in uas.bulk_in.iter().enumerate() {
            if idx > 0 {
                bulk_in.push(',');
            }
            bulk_in
                .push_str(alloc::format!("0x{:02X}/{}", ep.address, ep.max_packet_size).as_str());
        }

        let mut bulk_out = String::new();
        for (idx, ep) in uas.bulk_out.iter().enumerate() {
            if idx > 0 {
                bulk_out.push(',');
            }
            bulk_out
                .push_str(alloc::format!("0x{:02X}/{}", ep.address, ep.max_packet_size).as_str());
        }

        if let Some(target) = transport_plan.bot {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} uas candidate if#{} alt={} cfg={} in=[{}] out=[{}] bot_target if#{} alt={} proto={:02X}\n",
                vendor_id,
                product_id,
                uas.interface_number,
                uas.alternate_setting,
                uas.configuration_value,
                bulk_in,
                bulk_out,
                target.interface_number,
                target.alternate_setting,
                target.protocol,
            );
        } else {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} uas candidate if#{} alt={} cfg={} in=[{}] out=[{}] bot_target=none\n",
                vendor_id,
                product_id,
                uas.interface_number,
                uas.alternate_setting,
                uas.configuration_value,
                bulk_in,
                bulk_out,
            );
        }
    }

    if SKHYNIX_USE_UAS && is_skhynix_pssd_x31(vendor_id, product_id) {
        let Some(uas_target) =
            mass::pick_skhynix_uas_target(vendor_id, product_id, &transport_plan.uas)
        else {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} uas-skhynix selected but fixed endpoint target is missing; no bot fallback\n",
                vendor_id,
                product_id
            );
            return true;
        };

        let device = match host.open_device(dev_info).await {
            Ok(device) => device,
            Err(err) => {
                crate::log!(
                    "crabusb: mass {:04X}:{:04X} uas-skhynix open failed: {:?}\n",
                    vendor_id,
                    product_id,
                    err
                );
                return true;
            }
        };

        let active_stream = ActiveMassStream {
            controller_id,
            slot_id: u32::from(device.slot_id()),
        };
        if !register_active_mass_stream(active_stream) {
            return true;
        }

        match mass_storage_uas_skhynix_task(
            device,
            controller_id,
            uas_target,
            topology.port_speed,
            uas_candidate_count,
        ) {
            Ok(token) => {
                spawner.spawn(token);
                crate::log!(
                    "crabusb: mass {:04X}:{:04X} uas-skhynix handoff if#{} alt={} cfg={} cmd_out=0x{:02X} status_in=0x{:02X} data_in=0x{:02X} data_out=0x{:02X} transport={} profile={} speed={:?} uas_candidates={}\n",
                    vendor_id,
                    product_id,
                    uas_target.interface_number,
                    uas_target.alternate_setting,
                    uas_target.configuration_value,
                    uas_target.command_out,
                    uas_target.status_in,
                    uas_target.data_in,
                    uas_target.data_out,
                    mass_transport_label(mass::MassTransportKind::Uas),
                    mass_io_profile_label(MassIoProfile::UasSkhynix),
                    topology.port_speed,
                    uas_candidate_count,
                );
            }
            Err(err) => {
                unregister_active_mass_stream(active_stream);
                crate::log!(
                    "crabusb: mass {:04X}:{:04X} uas-skhynix spawn failed if#{} alt={}: {:?}\n",
                    vendor_id,
                    product_id,
                    uas_target.interface_number,
                    uas_target.alternate_setting,
                    err
                );
            }
        }

        return true;
    }

    let Some(target) = transport_plan.bot else {
        return false;
    };
    let transport_kind = mass::MassTransportKind::Bot;
    let io_profile =
        choose_mass_io_profile(transport_kind, vendor_id, product_id, topology.port_speed, &target);

    let device = match host.open_device(dev_info).await {
        Ok(device) => device,
        Err(err) => {
            crate::log!(
                "crabusb: mass {:04X}:{:04X} open failed: {:?}\n",
                vendor_id,
                product_id,
                err
            );
            return true;
        }
    };

    let active_stream = ActiveMassStream {
        controller_id,
        slot_id: u32::from(device.slot_id()),
    };
    if !register_active_mass_stream(active_stream) {
        return true;
    }

    match mass_storage_task(
        device,
        controller_id,
        target,
        transport_kind,
        io_profile,
        topology.port_speed,
        uas_candidate_count,
    ) {
        Ok(token) => {
            spawner.spawn(token);
            crate::log!(
                "crabusb: mass {:04X}:{:04X} handoff if#{} alt={} cfg={} bulk_in=0x{:02X} bulk_out=0x{:02X} in_mps={} out_mps={} transport={} profile={} speed={:?} uas_candidates={}\n",
                vendor_id,
                product_id,
                target.interface_number,
                target.alternate_setting,
                target.configuration_value,
                target.bulk_in,
                target.bulk_out,
                target.bulk_in_max_packet_size,
                target.bulk_out_max_packet_size,
                mass_transport_label(transport_kind),
                mass_io_profile_label(io_profile),
                topology.port_speed,
                uas_candidate_count,
            );
        }
        Err(err) => {
            unregister_active_mass_stream(active_stream);
            crate::log!(
                "crabusb: mass {:04X}:{:04X} spawn failed if#{} alt={}: {:?}\n",
                vendor_id,
                product_id,
                target.interface_number,
                target.alternate_setting,
                err
            );
        }
    }

    true
}
