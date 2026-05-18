use alloc::vec::Vec;
use core::{
    sync::atomic::{AtomicU64, Ordering},
    task::Poll,
};
use crab_usb::{EndpointBulkIn, EndpointBulkOut, usb_if};
use embassy_time::{Duration as EmbassyDuration, Timer};

use super::super::scsi;
use super::{
    MassProbeError, MassProbeInfo, UAS_IO_TIMEOUT_MS, UAS_STATUS_GRACE_MS, USB_CLASS_MASS_STORAGE,
    USB_PROTO_UAS, USB_SUBCLASS_SCSI, decode_ascii_field, with_timeout_or_none,
};

const UAS_IU_COMMAND: u8 = 0x01;
const UAS_IU_STATUS: u8 = 0x03;
const UAS_IU_READ_READY: u8 = 0x06;
const UAS_IU_WRITE_READY: u8 = 0x07;
const UAS_STATUS_GOOD: u8 = 0x00;
const UAS_XHCI_STREAM_COUNT: u16 = 32;
pub(crate) const UAS_XHCI_MAX_STREAM_ID: u16 = UAS_XHCI_STREAM_COUNT - 1;
static UAS_TRACE_DEBUG_LAST_LOG_TICK: AtomicU64 = AtomicU64::new(0);
static UAS_TRACE_IU_LAST_LOG_TICK: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub(crate) struct MassBulkEndpoint {
    pub address: u8,
    pub max_packet_size: u16,
}

#[derive(Clone, Debug)]
pub(crate) struct UasCandidate {
    pub configuration_value: u8,
    pub interface_number: u8,
    pub alternate_setting: u8,
    pub bulk_in: Vec<MassBulkEndpoint>,
    pub bulk_out: Vec<MassBulkEndpoint>,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct UasTarget {
    pub configuration_value: u8,
    pub interface_number: u8,
    pub alternate_setting: u8,
    pub command_out: u8,
    pub status_in: u8,
    pub data_in: u8,
    pub data_out: u8,
    pub command_out_max_packet_size: u16,
    pub status_in_max_packet_size: u16,
    pub data_in_max_packet_size: u16,
    pub data_out_max_packet_size: u16,
}

pub(super) fn collect_uas_candidates(
    configs: &[usb_if::descriptor::ConfigurationDescriptor],
) -> Vec<UasCandidate> {
    let mut out = Vec::new();

    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                if alt.class != USB_CLASS_MASS_STORAGE
                    || alt.subclass != USB_SUBCLASS_SCSI
                    || alt.protocol != USB_PROTO_UAS
                {
                    continue;
                }

                let mut bulk_in = Vec::new();
                let mut bulk_out = Vec::new();
                for ep in alt.endpoints.iter() {
                    if ep.transfer_type != usb_if::descriptor::EndpointType::Bulk {
                        continue;
                    }

                    let item = MassBulkEndpoint {
                        address: ep.address,
                        max_packet_size: ep.max_packet_size,
                    };
                    match ep.direction {
                        usb_if::transfer::Direction::In => bulk_in.push(item),
                        usb_if::transfer::Direction::Out => bulk_out.push(item),
                    }
                }

                if bulk_in.is_empty() || bulk_out.is_empty() {
                    continue;
                }

                out.push(UasCandidate {
                    configuration_value: config.configuration_value,
                    interface_number: interface.interface_number,
                    alternate_setting: alt.alternate_setting,
                    bulk_in,
                    bulk_out,
                });
            }
        }
    }

    out
}

pub(crate) fn pick_skhynix_uas_target(
    vendor_id: u16,
    product_id: u16,
    candidates: &[UasCandidate],
) -> Option<UasTarget> {
    if vendor_id != 0x152E || product_id != 0x7001 {
        return None;
    }

    for uas in candidates.iter() {
        let command_out = uas.bulk_out.iter().find(|ep| ep.address == 0x04)?;
        let status_in = uas.bulk_in.iter().find(|ep| ep.address == 0x83)?;
        let data_out = uas.bulk_out.iter().find(|ep| ep.address == 0x02)?;
        let data_in = uas.bulk_in.iter().find(|ep| ep.address == 0x81)?;
        return Some(UasTarget {
            configuration_value: uas.configuration_value,
            interface_number: uas.interface_number,
            alternate_setting: uas.alternate_setting,
            command_out: command_out.address,
            status_in: status_in.address,
            data_in: data_in.address,
            data_out: data_out.address,
            command_out_max_packet_size: command_out.max_packet_size,
            status_in_max_packet_size: status_in.max_packet_size,
            data_in_max_packet_size: data_in.max_packet_size,
            data_out_max_packet_size: data_out.max_packet_size,
        });
    }

    None
}

fn make_uas_command_iu(tag: u16, cdb: &[u8]) -> [u8; 32] {
    let mut iu = [0u8; 32];
    iu[0] = UAS_IU_COMMAND;
    iu[2..4].copy_from_slice(&tag.to_be_bytes());
    iu[4] = 0;
    iu[6] = 0;
    let cdb_len = cdb.len().min(16);
    iu[16..16 + cdb_len].copy_from_slice(&cdb[..cdb_len]);
    iu
}

#[allow(dead_code)]
fn cdb_write_buffer_echo(len: usize) -> [u8; 10] {
    let len = len.min(0x00FF_FFFF);
    [
        0x3B,
        0x0A,
        0,
        0,
        0,
        0,
        ((len >> 16) & 0xFF) as u8,
        ((len >> 8) & 0xFF) as u8,
        (len & 0xFF) as u8,
        0,
    ]
}

#[allow(dead_code)]
fn cdb_read_buffer_echo(len: usize) -> [u8; 10] {
    let len = len.min(0x00FF_FFFF);
    [
        0x3C,
        0x0A,
        0,
        0,
        0,
        0,
        ((len >> 16) & 0xFF) as u8,
        ((len >> 8) & 0xFF) as u8,
        (len & 0xFF) as u8,
        0,
    ]
}

fn uas_tag(tag: u32) -> u16 {
    ((tag.max(1) - 1) % u32::from(UAS_XHCI_MAX_STREAM_ID) + 1) as u16
}

pub(crate) fn uas_stream_id_from_tag(tag: u32) -> u16 {
    uas_tag(tag)
}

fn parse_uas_tag(iu: &[u8]) -> Option<u16> {
    if iu.len() < 4 {
        return None;
    }
    Some(u16::from_be_bytes([iu[2], iu[3]]))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UasReadStatusKind {
    ReadReady,
    StatusGood,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UasWriteStatusKind {
    WriteReady,
    StatusGood,
}

fn validate_uas_status(
    _cmd: &'static str,
    iu: &[u8],
    expected_tag: u16,
) -> Result<(), MassProbeError> {
    if iu.len() < 10 {
        return Err(MassProbeError::ShortData);
    }
    let iu_id = iu[0];
    let tag = parse_uas_tag(iu).unwrap_or(0);
    let status = iu[6];
    if iu_id != UAS_IU_STATUS || tag != expected_tag || status != UAS_STATUS_GOOD {
        return Err(MassProbeError::Csw);
    }
    Ok(())
}

pub(crate) fn classify_uas_read_status_iu(
    cmd: &'static str,
    iu: &[u8],
    expected_tag: u16,
) -> Result<UasReadStatusKind, MassProbeError> {
    if iu.len() < 4 {
        return Err(MassProbeError::ShortData);
    }

    let iu_id = iu[0];
    let tag = parse_uas_tag(iu).unwrap_or(0);
    if tag != expected_tag {
        return Err(MassProbeError::Csw);
    }

    match iu_id {
        UAS_IU_READ_READY => Ok(UasReadStatusKind::ReadReady),
        UAS_IU_STATUS => {
            validate_uas_status(cmd, iu, expected_tag)?;
            Ok(UasReadStatusKind::StatusGood)
        }
        _ => Err(MassProbeError::Csw),
    }
}

pub(crate) fn classify_uas_write_status_iu(
    cmd: &'static str,
    iu: &[u8],
    expected_tag: u16,
) -> Result<UasWriteStatusKind, MassProbeError> {
    if iu.len() < 4 {
        return Err(MassProbeError::ShortData);
    }

    let iu_id = iu[0];
    let tag = parse_uas_tag(iu).unwrap_or(0);
    if tag != expected_tag {
        return Err(MassProbeError::Csw);
    }

    match iu_id {
        UAS_IU_WRITE_READY => Ok(UasWriteStatusKind::WriteReady),
        UAS_IU_STATUS => {
            validate_uas_status(cmd, iu, expected_tag)?;
            Ok(UasWriteStatusKind::StatusGood)
        }
        _ => Err(MassProbeError::Csw),
    }
}

fn uas_trace_logs_enabled() -> bool {
    crate::logflag::USB_MASS_UAS_TRACE_LOGS
}

fn uas_trace_is_steady_keepalive(cmd: &'static str, stage: &'static str) -> bool {
    cmd == "test-unit-ready" && matches!(stage, "command-iu" | "status-submit" | "status-iu")
}

fn uas_trace_is_good_write_ready_substitute(
    cmd: &'static str,
    stage: &'static str,
    tag: u16,
    iu: &[u8],
) -> bool {
    cmd == "write-10" && stage == "ready-iu" && validate_uas_status(cmd, iu, tag).is_ok()
}

fn uas_trace_rate_limit_allows(last_marker: &AtomicU64) -> bool {
    let interval = embassy_time_driver::TICK_HZ.max(1);
    let now = embassy_time_driver::now();
    let now_marker = now.saturating_add(1);
    let mut previous_marker = last_marker.load(Ordering::Relaxed);

    loop {
        if previous_marker != 0 {
            let previous = previous_marker.saturating_sub(1);
            if now >= previous && now.saturating_sub(previous) < interval {
                return false;
            }
        }

        match last_marker.compare_exchange_weak(
            previous_marker,
            now_marker,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return true,
            Err(actual) => previous_marker = actual,
        }
    }
}

fn uas_trace_stage_always_log(stage: &'static str) -> bool {
    matches!(
        stage,
        "write-ready" | "final-status-submit" | "data-out-complete" | "final-status-complete"
    ) || stage.contains("timeout")
        || stage.contains("error")
        || stage.contains("short")
}

fn uas_trace_iu_stage_always_log(stage: &'static str) -> bool {
    matches!(stage, "ready-iu" | "final-status-iu")
}

fn log_uas_debug(stage: &'static str, cmd: &'static str, tag: u16) {
    if !uas_trace_logs_enabled() {
        return;
    }
    if uas_trace_is_steady_keepalive(cmd, stage) {
        return;
    }
    if !uas_trace_stage_always_log(stage)
        && !uas_trace_rate_limit_allows(&UAS_TRACE_DEBUG_LAST_LOG_TICK)
    {
        return;
    }

    let submit = crab_usb::debug_last_submit();
    let event = crab_usb::debug_last_event();
    let stream_cfg = crab_usb::debug_last_stream_config();
    crate::globalog::log_with_level(
        log::Level::Trace,
        format_args!(
            "crabusb: mass uas-debug stage={} cmd={} tag=0x{:04X} last_submit[slot={} dci={} dir={} stream={} len={} ptr=0x{:X} ring=0x{:X}] last_stream_cfg[slot={} dci={} ep=0x{:02X} count={} maxp={} burst={} mps={} ctx=0x{:X} ring1=0x{:X}] last_event[slot={} ep={} cc={} residual={} ptr=0x{:X}]\n",
            stage,
            cmd,
            tag,
            submit.slot_id,
            submit.dci,
            submit.direction,
            submit.stream_id,
            submit.len,
            submit.ptr,
            submit.ring_ptr,
            stream_cfg.slot_id,
            stream_cfg.dci,
            stream_cfg.ep_addr,
            stream_cfg.stream_count,
            stream_cfg.max_primary_streams,
            stream_cfg.max_burst,
            stream_cfg.max_packet_size,
            stream_cfg.ctx_ptr,
            stream_cfg.ring1_ptr,
            event.slot_id,
            event.ep_id,
            event.completion_code,
            event.residual,
            event.ptr
        ),
    );
}

fn log_uas_iu(stage: &'static str, cmd: &'static str, tag: u16, iu: &[u8]) {
    if !uas_trace_logs_enabled() {
        return;
    }
    if uas_trace_is_steady_keepalive(cmd, stage) {
        return;
    }
    if uas_trace_is_good_write_ready_substitute(cmd, stage, tag, iu) {
        return;
    }
    if !uas_trace_iu_stage_always_log(stage)
        && !uas_trace_rate_limit_allows(&UAS_TRACE_IU_LAST_LOG_TICK)
    {
        return;
    }

    let mut bytes = [0u8; 16];
    let n = iu.len().min(bytes.len());
    bytes[..n].copy_from_slice(&iu[..n]);
    crate::globalog::log_with_level(
        log::Level::Trace,
        format_args!(
            "crabusb: mass uas-iu stage={} cmd={} tag=0x{:04X} len={} bytes={:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}\n",
            stage,
            cmd,
            tag,
            iu.len(),
            bytes[0],
            bytes[1],
            bytes[2],
            bytes[3],
            bytes[4],
            bytes[5],
            bytes[6],
            bytes[7],
            bytes[8],
            bytes[9],
            bytes[10],
            bytes[11],
            bytes[12],
            bytes[13],
            bytes[14],
            bytes[15]
        ),
    );
}

async fn uas_send_command(
    command_out: &mut EndpointBulkOut,
    cmd: &'static str,
    cdb: &[u8],
    tag: u16,
) -> Result<(), MassProbeError> {
    let iu = make_uas_command_iu(tag, cdb);
    log_uas_iu("command-iu", cmd, tag, &iu);
    let sent = with_timeout_or_none(command_out.submit_and_wait(&iu), UAS_IO_TIMEOUT_MS)
        .await
        .ok_or_else(|| {
            log_uas_debug("command-timeout", cmd, tag);
            MassProbeError::Transport("uas-command-timeout")
        })?
        .map_err(|_| MassProbeError::Transport("uas-command-out"))?;
    log_uas_debug("command-complete", cmd, tag);
    if sent != iu.len() {
        return Err(MassProbeError::ShortData);
    }
    Ok(())
}

pub(crate) async fn send_read10_uas_skhynix(
    command_out: &mut EndpointBulkOut,
    lba: u32,
    blocks: u16,
    tag: u32,
) -> Result<u16, MassProbeError> {
    let tag = uas_tag(tag);
    let cdb = scsi::cdb_read_10(lba, blocks);
    uas_send_command(command_out, "read-10", &cdb, tag).await?;
    Ok(tag)
}

pub(crate) async fn send_write10_uas_skhynix(
    command_out: &mut EndpointBulkOut,
    lba: u32,
    blocks: u16,
    tag: u32,
) -> Result<u16, MassProbeError> {
    let tag = uas_tag(tag);
    let cdb = scsi::cdb_write_10(lba, blocks);
    uas_send_command(command_out, "write-10", &cdb, tag).await?;
    Ok(tag)
}

async fn uas_drain_status_grace(
    status_in: &mut EndpointBulkIn,
    cmd: &'static str,
    tag: u16,
) -> Result<(), MassProbeError> {
    let mut status = [0u8; 96];
    match with_timeout_or_none(
        status_in.submit_on_stream_and_wait(tag, &mut status),
        UAS_STATUS_GRACE_MS,
    )
    .await
    {
        Some(Ok(got)) => {
            if got < 4 {
                log_uas_debug("status-short-after-ready-data", cmd, tag);
                return Ok(());
            }
            let status = &status[..got.min(status.len())];
            log_uas_iu("status-iu-after-ready-data", cmd, tag, status);
            validate_uas_status(cmd, status, tag)?;
            log_uas_debug("status-after-ready-data", cmd, tag);
        }
        Some(Err(_)) => log_uas_debug("status-error-after-ready-data", cmd, tag),
        None => log_uas_debug("status-timeout-after-ready-data", cmd, tag),
    }
    Ok(())
}

async fn uas_command_in(
    command_out: &mut EndpointBulkOut,
    status_in: &mut EndpointBulkIn,
    data_in: &mut EndpointBulkIn,
    cmd: &'static str,
    cdb: &[u8],
    data: &mut [u8],
    tag: u32,
) -> Result<usize, MassProbeError> {
    let tag = uas_tag(tag);
    let mut ready_iu = [0u8; 16];
    let ready_handle = status_in
        .submit_on_stream(tag, &mut ready_iu)
        .map_err(|_| MassProbeError::Transport("uas-status-submit"))?;
    log_uas_debug("status-submit", cmd, tag);
    let data_handle = data_in
        .submit_on_stream(tag, data)
        .map_err(|_| MassProbeError::Transport("uas-data-submit"))?;
    log_uas_debug("data-in-submit", cmd, tag);
    uas_send_command(command_out, cmd, cdb, tag).await?;

    enum FirstInCompletion {
        Status(Result<usize, MassProbeError>),
        Data(Result<usize, MassProbeError>),
        Timeout,
    }

    let mut ready_handle = core::pin::pin!(ready_handle);
    let mut data_handle = core::pin::pin!(data_handle);
    let mut timeout =
        core::pin::pin!(Timer::after(EmbassyDuration::from_millis(UAS_IO_TIMEOUT_MS)));
    let first = core::future::poll_fn(|cx| {
        if let Poll::Ready(result) = data_handle.as_mut().poll(cx) {
            return Poll::Ready(FirstInCompletion::Data(
                result
                    .map(|transfer| transfer.transfer_len)
                    .map_err(|_| MassProbeError::Transport("uas-data-in")),
            ));
        }
        if let Poll::Ready(result) = ready_handle.as_mut().poll(cx) {
            return Poll::Ready(FirstInCompletion::Status(
                result
                    .map(|transfer| transfer.transfer_len)
                    .map_err(|_| MassProbeError::Transport("uas-status-in")),
            ));
        }
        if timeout.as_mut().poll(cx).is_ready() {
            return Poll::Ready(FirstInCompletion::Timeout);
        }
        Poll::Pending
    })
    .await;

    match first {
        FirstInCompletion::Data(got) => {
            let got = got?;
            log_uas_debug("data-in-complete-before-status", cmd, tag);
            match with_timeout_or_none(ready_handle.as_mut(), UAS_STATUS_GRACE_MS).await {
                Some(Ok(transfer)) => {
                    let ready_got = transfer.transfer_len;
                    if ready_got >= 4 {
                        let ready = &ready_iu[..ready_got.min(ready_iu.len())];
                        log_uas_iu("post-data-iu", cmd, tag, ready);
                        let ready_id = ready[0];
                        let ready_tag = parse_uas_tag(ready).unwrap_or(0);
                        if ready_id == UAS_IU_STATUS && ready_tag == tag {
                            validate_uas_status(cmd, ready, tag)?;
                            log_uas_debug("status-after-data", cmd, tag);
                        } else if ready_id == UAS_IU_READ_READY && ready_tag == tag {
                            log_uas_debug("ready-after-data", cmd, tag);
                            uas_drain_status_grace(status_in, cmd, tag).await?;
                        } else {
                            return Err(MassProbeError::Csw);
                        }
                    } else {
                        log_uas_debug("status-short-after-data", cmd, tag);
                    }
                }
                Some(Err(_)) => log_uas_debug("status-error-after-data", cmd, tag),
                None => log_uas_debug("status-timeout-after-data", cmd, tag),
            }
            Ok(got)
        }
        FirstInCompletion::Status(ready_got) => {
            let ready_got = ready_got?;
            if ready_got < 4 {
                return Err(MassProbeError::ShortData);
            }
            let ready = &ready_iu[..ready_got.min(ready_iu.len())];
            log_uas_iu("ready-iu", cmd, tag, ready);
            let ready_id = ready[0];
            let ready_tag = parse_uas_tag(ready).unwrap_or(0);
            if ready_id == UAS_IU_STATUS && ready_tag == tag {
                validate_uas_status(cmd, ready, tag)?;
                log_uas_debug("status-before-data", cmd, tag);
                let got = with_timeout_or_none(data_handle.as_mut(), UAS_IO_TIMEOUT_MS)
                    .await
                    .ok_or_else(|| {
                        log_uas_debug("data-in-timeout-after-status", cmd, tag);
                        MassProbeError::Transport("uas-data-timeout")
                    })?
                    .map_err(|_| MassProbeError::Transport("uas-data-in"))?
                    .transfer_len;
                log_uas_debug("data-in-complete-after-status", cmd, tag);
                return Ok(got);
            }
            if ready_id != UAS_IU_READ_READY || ready_tag != tag {
                return Err(MassProbeError::Csw);
            }
            log_uas_debug("ready-before-data", cmd, tag);
            let got = with_timeout_or_none(data_handle.as_mut(), UAS_IO_TIMEOUT_MS)
                .await
                .ok_or_else(|| {
                    log_uas_debug("data-in-timeout-after-ready", cmd, tag);
                    MassProbeError::Transport("uas-data-timeout")
                })?
                .map_err(|_| MassProbeError::Transport("uas-data-in"))?
                .transfer_len;
            log_uas_debug("data-in-complete-after-ready", cmd, tag);
            uas_drain_status_grace(status_in, cmd, tag).await?;
            Ok(got)
        }
        FirstInCompletion::Timeout => {
            log_uas_debug("in-timeout", cmd, tag);
            Err(MassProbeError::Transport("uas-in-timeout"))
        }
    }
}

async fn uas_command_out(
    command_out: &mut EndpointBulkOut,
    status_in: &mut EndpointBulkIn,
    data_out: &mut EndpointBulkOut,
    cmd: &'static str,
    cdb: &[u8],
    data: &[u8],
    tag: u32,
) -> Result<(), MassProbeError> {
    let tag = uas_tag(tag);
    let mut ready_iu = [0u8; 16];
    let ready_handle = status_in
        .submit_on_stream(tag, &mut ready_iu)
        .map_err(|_| MassProbeError::Transport("uas-status-submit"))?;
    log_uas_debug("status-submit", cmd, tag);
    uas_send_command(command_out, cmd, cdb, tag).await?;

    let mut ready_handle = core::pin::pin!(ready_handle);
    let ready_got = with_timeout_or_none(ready_handle.as_mut(), UAS_IO_TIMEOUT_MS)
        .await
        .ok_or_else(|| {
            log_uas_debug("ready-timeout", cmd, tag);
            MassProbeError::Transport("uas-status-timeout")
        })?
        .map_err(|_| MassProbeError::Transport("uas-status-in"))?
        .transfer_len;
    if ready_got < 4 {
        return Err(MassProbeError::ShortData);
    }

    let ready = &ready_iu[..ready_got.min(ready_iu.len())];
    log_uas_iu("ready-iu", cmd, tag, ready);
    let ready_id = ready[0];
    let ready_tag = parse_uas_tag(ready).unwrap_or(0);
    if ready_tag != tag {
        return Err(MassProbeError::Csw);
    }
    if ready_id == UAS_IU_STATUS {
        validate_uas_status(cmd, ready, tag)?;
        log_uas_debug("status-before-write-ready", cmd, tag);
        return Err(MassProbeError::Csw);
    }
    if ready_id != UAS_IU_WRITE_READY {
        return Err(MassProbeError::Csw);
    }
    log_uas_debug("write-ready", cmd, tag);

    let data_handle = data_out
        .submit_on_stream(tag, data)
        .map_err(|_| MassProbeError::Transport("uas-data-submit"))?;
    log_uas_debug("data-out-submit", cmd, tag);

    let data_sent = with_timeout_or_none(data_handle, UAS_IO_TIMEOUT_MS)
        .await
        .ok_or_else(|| {
            log_uas_debug("data-out-timeout", cmd, tag);
            MassProbeError::Transport("uas-data-timeout")
        })?
        .map_err(|_| MassProbeError::Transport("uas-data-out"))?
        .transfer_len;
    log_uas_debug("data-out-complete", cmd, tag);

    if data_sent != data.len() {
        return Err(MassProbeError::ShortData);
    }

    let mut final_status = [0u8; 96];
    let final_got = with_timeout_or_none(
        status_in.submit_on_stream_and_wait(tag, &mut final_status),
        UAS_IO_TIMEOUT_MS,
    )
    .await
    .ok_or_else(|| {
        log_uas_debug("final-status-timeout", cmd, tag);
        MassProbeError::Transport("uas-status-timeout")
    })?
    .map_err(|_| MassProbeError::Transport("uas-status-in"))?;
    let status = &final_status[..final_got.min(final_status.len())];
    log_uas_iu("final-status-iu", cmd, tag, status);
    log_uas_debug("final-status-complete", cmd, tag);
    validate_uas_status(cmd, status, tag)
}

async fn uas_command_no_data(
    command_out: &mut EndpointBulkOut,
    status_in: &mut EndpointBulkIn,
    cmd: &'static str,
    cdb: &[u8],
    tag: u32,
) -> Result<(), MassProbeError> {
    let tag = uas_tag(tag);
    let mut status = [0u8; 96];
    let status_handle = status_in
        .submit_on_stream(tag, &mut status)
        .map_err(|_| MassProbeError::Transport("uas-status-submit"))?;
    log_uas_debug("status-submit", cmd, tag);
    uas_send_command(command_out, cmd, cdb, tag).await?;
    let got = with_timeout_or_none(status_handle, UAS_IO_TIMEOUT_MS)
        .await
        .ok_or_else(|| {
            log_uas_debug("status-timeout", cmd, tag);
            MassProbeError::Transport("uas-status-timeout")
        })?
        .map_err(|_| MassProbeError::Transport("uas-status-in"))?
        .transfer_len;
    log_uas_iu("status-iu", cmd, tag, &status[..got.min(status.len())]);
    validate_uas_status(cmd, &status[..got.min(status.len())], tag)
}

pub(crate) async fn request_sense_fixed_uas_skhynix(
    command_out: &mut EndpointBulkOut,
    status_in: &mut EndpointBulkIn,
    data_in: &mut EndpointBulkIn,
    tag: u32,
) -> Option<scsi::SenseFixed> {
    request_sense_fixed_uas_skhynix_result(command_out, status_in, data_in, tag)
        .await
        .ok()
        .flatten()
}

pub(crate) async fn request_sense_fixed_uas_skhynix_result(
    command_out: &mut EndpointBulkOut,
    status_in: &mut EndpointBulkIn,
    data_in: &mut EndpointBulkIn,
    tag: u32,
) -> Result<Option<scsi::SenseFixed>, MassProbeError> {
    let mut sense = [0u8; 18];
    let cdb = scsi::cdb_request_sense(18);
    let got =
        uas_command_in(command_out, status_in, data_in, "request-sense", &cdb, &mut sense, tag)
            .await?;

    Ok(scsi::parse_request_sense_fixed(&sense[..got.min(sense.len())]))
}

pub(crate) async fn keepalive_mass_uas_skhynix(
    command_out: &mut EndpointBulkOut,
    status_in: &mut EndpointBulkIn,
    tag: u32,
) -> Result<(), MassProbeError> {
    let cdb = scsi::cdb_test_unit_ready();
    uas_command_no_data(command_out, status_in, "test-unit-ready", &cdb, tag).await
}

pub(crate) async fn read_blocks_uas_skhynix(
    command_out: &mut EndpointBulkOut,
    status_in: &mut EndpointBulkIn,
    data_in: &mut EndpointBulkIn,
    lba: u32,
    blocks: u16,
    out: &mut [u8],
    tag: u32,
) -> Result<(), MassProbeError> {
    let cdb = scsi::cdb_read_10(lba, blocks);
    let got = uas_command_in(command_out, status_in, data_in, "read-10", &cdb, out, tag).await?;
    if got < out.len() {
        return Err(MassProbeError::ShortData);
    }
    Ok(())
}

pub(crate) async fn write_blocks_uas_skhynix(
    command_out: &mut EndpointBulkOut,
    status_in: &mut EndpointBulkIn,
    data_out: &mut EndpointBulkOut,
    lba: u32,
    blocks: u16,
    data: &[u8],
    tag: u32,
) -> Result<(), MassProbeError> {
    let cdb = scsi::cdb_write_10(lba, blocks);
    uas_command_out(command_out, status_in, data_out, "write-10", &cdb, data, tag).await
}

pub(crate) async fn synchronize_cache_uas_skhynix(
    command_out: &mut EndpointBulkOut,
    status_in: &mut EndpointBulkIn,
    tag: u32,
) -> Result<(), MassProbeError> {
    let cdb = scsi::cdb_synchronize_cache_10();
    uas_command_no_data(command_out, status_in, "sync-cache-10", &cdb, tag).await
}

#[allow(dead_code)]
async fn echo_buffer_uas_skhynix(
    command_out: &mut EndpointBulkOut,
    status_in: &mut EndpointBulkIn,
    data_in: &mut EndpointBulkIn,
    data_out: &mut EndpointBulkOut,
) -> Result<(), MassProbeError> {
    let pattern = [
        0x54, 0x52, 0x55, 0x45, 0x4F, 0x53, 0x2D, 0x55, 0x41, 0x53, 0x2D, 0x45, 0x43, 0x48, 0x4F,
        0x21,
    ];
    let write_cdb = cdb_write_buffer_echo(pattern.len());
    uas_command_out(
        command_out,
        status_in,
        data_out,
        "write-buffer-echo",
        &write_cdb,
        &pattern,
        0x5541_EC01,
    )
    .await?;

    let mut echoed = [0u8; 16];
    let read_cdb = cdb_read_buffer_echo(echoed.len());
    let got = uas_command_in(
        command_out,
        status_in,
        data_in,
        "read-buffer-echo",
        &read_cdb,
        &mut echoed,
        0x5541_EC02,
    )
    .await?;
    if got < pattern.len() {
        return Err(MassProbeError::ShortData);
    }
    if echoed != pattern {
        return Err(MassProbeError::Transport("uas-echo-mismatch"));
    }
    Ok(())
}

pub(crate) async fn exercise_mass_uas_skhynix(
    command_out: &mut EndpointBulkOut,
    status_in: &mut EndpointBulkIn,
    data_in: &mut EndpointBulkIn,
    _data_out: &mut EndpointBulkOut,
) -> Result<MassProbeInfo, MassProbeError> {
    let info = probe_mass_uas_skhynix(command_out, status_in, data_in).await?;
    Ok(info)
}

async fn read_capacity_16_uas_skhynix(
    command_out: &mut EndpointBulkOut,
    status_in: &mut EndpointBulkIn,
    data_in: &mut EndpointBulkIn,
) -> Result<(u64, u32), MassProbeError> {
    let mut read_capacity = [0u8; 32];
    let alloc_len = read_capacity.len() as u32;
    let read_capacity_cdb = [
        0x9E,
        0x10,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        (alloc_len >> 24) as u8,
        (alloc_len >> 16) as u8,
        (alloc_len >> 8) as u8,
        alloc_len as u8,
        0,
        0,
    ];
    let got = uas_command_in(
        command_out,
        status_in,
        data_in,
        "read-capacity16",
        &read_capacity_cdb,
        &mut read_capacity,
        0x5541_4E53,
    )
    .await?;
    if got < 12 {
        return Err(MassProbeError::ShortData);
    }
    let last_lba = u64::from_be_bytes([
        read_capacity[0],
        read_capacity[1],
        read_capacity[2],
        read_capacity[3],
        read_capacity[4],
        read_capacity[5],
        read_capacity[6],
        read_capacity[7],
    ]);
    let block_size = u32::from_be_bytes([
        read_capacity[8],
        read_capacity[9],
        read_capacity[10],
        read_capacity[11],
    ]);
    Ok((last_lba + 1, block_size))
}

pub(crate) async fn probe_mass_uas_skhynix(
    command_out: &mut EndpointBulkOut,
    status_in: &mut EndpointBulkIn,
    data_in: &mut EndpointBulkIn,
) -> Result<MassProbeInfo, MassProbeError> {
    let mut inquiry = [0u8; 36];
    let inquiry_cdb = scsi::cdb_inquiry(inquiry.len() as u16);
    let inquiry_read = uas_command_in(
        command_out,
        status_in,
        data_in,
        "inquiry",
        &inquiry_cdb,
        &mut inquiry,
        0x5541_4E51,
    )
    .await?;
    if inquiry_read < 32 {
        return Err(MassProbeError::ShortData);
    }

    let removable = (inquiry[1] & 0x80) != 0;
    crate::log_info!(target: "usb";
        "crabusb: mass uas-skhynix inquiry removable={} pdt=0x{:02X}\n",
        removable,
        inquiry[0] & 0x1F
    );

    let mut read_capacity = [0u8; 8];
    let read_capacity_cdb = scsi::cdb_read_capacity_10();
    let (block_count, block_size) = match uas_command_in(
        command_out,
        status_in,
        data_in,
        "read-capacity10",
        &read_capacity_cdb,
        &mut read_capacity,
        0x5541_4E52,
    )
    .await
    {
        Ok(read_capacity_read) => {
            if read_capacity_read < read_capacity.len() {
                return Err(MassProbeError::ShortData);
            }
            let last_lba = u32::from_be_bytes([
                read_capacity[0],
                read_capacity[1],
                read_capacity[2],
                read_capacity[3],
            ]);
            let block_size = u32::from_be_bytes([
                read_capacity[4],
                read_capacity[5],
                read_capacity[6],
                read_capacity[7],
            ]);
            (u64::from(last_lba) + 1, block_size)
        }
        Err(err) => {
            crate::log_info!(target: "usb";
                "crabusb: mass uas-skhynix read-capacity10 failed: {:?}; trying capacity16\n",
                err
            );
            let _ =
                request_sense_fixed_uas_skhynix(command_out, status_in, data_in, 0x5541_4E55).await;
            read_capacity_16_uas_skhynix(command_out, status_in, data_in).await?
        }
    };
    if block_size == 0 {
        return Err(MassProbeError::ShortData);
    }

    let vendor = decode_ascii_field(&inquiry[8..16]);
    let product = decode_ascii_field(&inquiry[16..32]);

    Ok(MassProbeInfo {
        block_size,
        block_count,
        vendor,
        product,
    })
}
