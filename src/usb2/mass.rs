use alloc::{boxed::Box, string::String, vec, vec::Vec};
use core::{
    future::Future,
    ptr::{read_unaligned, read_volatile},
    task::Poll,
};
use crab_usb::{EndpointBulkIn, EndpointBulkOut, err::TransferError, usb_if};
use embassy_time::{Duration as EmbassyDuration, Timer};
use usb_if::host::ControlSetup;
use usb_if::transfer::{Recipient, Request, RequestType};

use super::scsi;
use crate::disc::block;

const USB_CLASS_MASS_STORAGE: u8 = 0x08;
const USB_SUBCLASS_SCSI: u8 = 0x06;
const USB_PROTO_BULK_ONLY: u8 = 0x50;
const USB_PROTO_UAS: u8 = 0x62;
const BOT_IO_RETRIES: usize = 8;
const BOT_IO_TIMEOUT_MS: u64 = crate::allcaps::storage::USB_MASS_BOT_IO_TIMEOUT_MS;
const UAS_IO_TIMEOUT_MS: u64 = crate::allcaps::storage::USB_MASS_UAS_IO_TIMEOUT_MS;
const UAS_STATUS_GRACE_MS: u64 = 100;
const BOT_RECOVERY_SETTLE_MS: u64 = crate::allcaps::storage::USB_MASS_BOT_RECOVERY_SETTLE_MS;
const UAS_IU_COMMAND: u8 = 0x01;
const UAS_IU_STATUS: u8 = 0x03;
const UAS_IU_READ_READY: u8 = 0x06;
const UAS_IU_WRITE_READY: u8 = 0x07;
const UAS_STATUS_GOOD: u8 = 0x00;
const UAS_XHCI_STREAM_COUNT: u16 = 32;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MassTransportKind {
    Bot,
    Uas,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct MassTarget {
    pub configuration_value: u8,
    pub interface_number: u8,
    pub alternate_setting: u8,
    pub bulk_in: u8,
    pub bulk_out: u8,
    pub bulk_in_max_packet_size: u16,
    pub bulk_out_max_packet_size: u16,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
}

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

#[derive(Clone, Debug)]
pub(crate) struct MassTransportPlan {
    pub bot: Option<MassTarget>,
    pub uas: Vec<UasCandidate>,
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

fn collect_uas_candidates(
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

pub(crate) fn inspect_mass_transports(
    configs: &[usb_if::descriptor::ConfigurationDescriptor],
) -> MassTransportPlan {
    MassTransportPlan {
        bot: pick_mass_target(configs),
        uas: collect_uas_candidates(configs),
    }
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

pub(crate) fn pick_mass_target(
    configs: &[usb_if::descriptor::ConfigurationDescriptor],
) -> Option<MassTarget> {
    let mut best: Option<(u32, MassTarget)> = None;

    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                let mut bulk_in = None;
                let mut bulk_out = None;

                for ep in alt.endpoints.iter() {
                    if ep.transfer_type != usb_if::descriptor::EndpointType::Bulk {
                        continue;
                    }
                    match ep.direction {
                        usb_if::transfer::Direction::In if bulk_in.is_none() => {
                            bulk_in = Some((ep.address, ep.max_packet_size));
                        }
                        usb_if::transfer::Direction::Out if bulk_out.is_none() => {
                            bulk_out = Some((ep.address, ep.max_packet_size));
                        }
                        _ => {}
                    }
                }

                let (bulk_in_addr, bulk_in_mps) = match bulk_in {
                    Some(v) => v,
                    None => continue,
                };
                let (bulk_out_addr, bulk_out_mps) = match bulk_out {
                    Some(v) => v,
                    None => continue,
                };

                let mut score = 10u32;
                if alt.class == USB_CLASS_MASS_STORAGE {
                    score += 100;
                }
                if alt.subclass == USB_SUBCLASS_SCSI {
                    score += 50;
                }
                if alt.protocol == USB_PROTO_BULK_ONLY {
                    score += 50;
                }
                if alt.alternate_setting == 0 {
                    score += 10;
                }
                score += alt.endpoints.len() as u32;

                let target = MassTarget {
                    configuration_value: config.configuration_value,
                    interface_number: interface.interface_number,
                    alternate_setting: alt.alternate_setting,
                    bulk_in: bulk_in_addr,
                    bulk_out: bulk_out_addr,
                    bulk_in_max_packet_size: bulk_in_mps,
                    bulk_out_max_packet_size: bulk_out_mps,
                    class: alt.class,
                    subclass: alt.subclass,
                    protocol: alt.protocol,
                };

                match best {
                    Some((best_score, _)) if best_score >= score => {}
                    _ => best = Some((score, target)),
                }
            }
        }
    }

    best.map(|(_, target)| target).filter(|target| {
        target.class == USB_CLASS_MASS_STORAGE
            && target.subclass == USB_SUBCLASS_SCSI
            && target.protocol == USB_PROTO_BULK_ONLY
    })
}

#[derive(Clone, Debug)]
pub(crate) struct MassProbeInfo {
    pub max_lun: u8,
    pub block_size: u32,
    pub block_count: u64,
    pub vendor: String,
    pub product: String,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum MassProbeError {
    Transport(&'static str),
    ShortData {
        cmd: &'static str,
        got: usize,
        need: usize,
    },
    Csw {
        cmd: &'static str,
        sig: u32,
        tag: u32,
        expected_tag: u32,
        status: u8,
    },
}

impl MassProbeError {
    pub(crate) fn transport_reason(self) -> Option<&'static str> {
        match self {
            MassProbeError::Transport(reason) => Some(reason),
            _ => None,
        }
    }
}

fn make_cbw(tag: u32, data_len: u32, flags: u8, lun: u8, cdb: &[u8]) -> [u8; 31] {
    let mut cbw = [0u8; 31];
    cbw[0..4].copy_from_slice(&0x4342_5355u32.to_le_bytes());
    cbw[4..8].copy_from_slice(&tag.to_le_bytes());
    cbw[8..12].copy_from_slice(&data_len.to_le_bytes());
    cbw[12] = flags;
    cbw[13] = lun;
    let cdb_len = cdb.len().min(16) as u8;
    cbw[14] = cdb_len;
    cbw[15..15 + usize::from(cdb_len)].copy_from_slice(&cdb[..usize::from(cdb_len)]);
    cbw
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
    ((tag.max(1) - 1) % u32::from(UAS_XHCI_STREAM_COUNT - 1) + 1) as u16
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

fn validate_uas_status(
    cmd: &'static str,
    iu: &[u8],
    expected_tag: u16,
) -> Result<(), MassProbeError> {
    if iu.len() < 10 {
        return Err(MassProbeError::ShortData {
            cmd,
            got: iu.len(),
            need: 10,
        });
    }
    let iu_id = iu[0];
    let tag = parse_uas_tag(iu).unwrap_or(0);
    let status = iu[6];
    if iu_id != UAS_IU_STATUS || tag != expected_tag || status != UAS_STATUS_GOOD {
        return Err(MassProbeError::Csw {
            cmd,
            sig: u32::from(iu_id),
            tag: u32::from(tag),
            expected_tag: u32::from(expected_tag),
            status,
        });
    }
    Ok(())
}

pub(crate) fn classify_uas_read_status_iu(
    cmd: &'static str,
    iu: &[u8],
    expected_tag: u16,
) -> Result<UasReadStatusKind, MassProbeError> {
    if iu.len() < 4 {
        return Err(MassProbeError::ShortData {
            cmd,
            got: iu.len(),
            need: 4,
        });
    }

    let iu_id = iu[0];
    let tag = parse_uas_tag(iu).unwrap_or(0);
    if tag != expected_tag {
        return Err(MassProbeError::Csw {
            cmd,
            sig: u32::from(iu_id),
            tag: u32::from(tag),
            expected_tag: u32::from(expected_tag),
            status: 0xFF,
        });
    }

    match iu_id {
        UAS_IU_READ_READY => Ok(UasReadStatusKind::ReadReady),
        UAS_IU_STATUS => {
            validate_uas_status(cmd, iu, expected_tag)?;
            Ok(UasReadStatusKind::StatusGood)
        }
        _ => Err(MassProbeError::Csw {
            cmd,
            sig: u32::from(iu_id),
            tag: u32::from(tag),
            expected_tag: u32::from(expected_tag),
            status: 0xFF,
        }),
    }
}

async fn with_timeout_or_none<F: Future>(fut: F, timeout_ms: u64) -> Option<F::Output> {
    let mut fut = core::pin::pin!(fut);
    let mut timeout = core::pin::pin!(Timer::after(EmbassyDuration::from_millis(timeout_ms)));

    core::future::poll_fn(|cx| {
        if let Poll::Ready(out) = fut.as_mut().poll(cx) {
            return Poll::Ready(Some(out));
        }
        if timeout.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        Poll::Pending
    })
    .await
}

async fn control_out_with_timeout(
    device: &mut crab_usb::Device,
    setup: ControlSetup,
    data: &[u8],
    timeout_ms: u64,
) -> Option<Result<usize, TransferError>> {
    with_timeout_or_none(device.control_out(setup, data), timeout_ms).await
}

fn log_transport_debug(stage: &'static str) {
    let submit = crab_usb::debug_last_submit();
    let event = crab_usb::debug_last_event();
    crate::log!(
        "crabusb: mass debug stage={} last_submit[slot={} dci={} dir={} stream={} len={} ptr=0x{:X} ring=0x{:X}] last_event[slot={} ep={} cc={} residual={} ptr=0x{:X}]\n",
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
}

fn log_bot_transport_debug(
    stage: &'static str,
    cmd: &'static str,
    tag: u32,
    ep: u8,
    direction: u8,
    len: usize,
    ptr: *const u8,
) {
    let submit = crab_usb::debug_last_submit();
    let event = crab_usb::debug_last_event();
    crate::log!(
        "crabusb: mass bot-debug stage={} cmd={} tag=0x{:08X} expect[dci={} ep=0x{:02X} dir={} len={} ptr=0x{:X}] last_submit[slot={} dci={} dir={} stream={} len={} ptr=0x{:X} ring=0x{:X}] last_event[slot={} ep={} cc={} residual={} ptr=0x{:X}]\n",
        stage,
        cmd,
        tag,
        endpoint_dci(ep),
        ep,
        direction,
        len,
        ptr as usize,
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

fn uas_probe_logs_enabled() -> bool {
    crate::logflag::USB_MASS_UAS_ADVANCED_PROBE_LOGS
}

fn log_uas_debug(stage: &'static str, cmd: &'static str, tag: u16) {
    if !uas_probe_logs_enabled() {
        return;
    }

    let submit = crab_usb::debug_last_submit();
    let event = crab_usb::debug_last_event();
    let stream_cfg = crab_usb::debug_last_stream_config();
    crate::log!(
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
    );
}

fn log_uas_iu(stage: &'static str, cmd: &'static str, tag: u16, iu: &[u8]) {
    if !uas_probe_logs_enabled() {
        return;
    }

    let mut bytes = [0u8; 16];
    let n = iu.len().min(bytes.len());
    bytes[..n].copy_from_slice(&iu[..n]);
    crate::log!(
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
    );
}

fn read_mmio_u32(base: *const u8, offset: usize) -> u32 {
    unsafe { read_volatile(base.add(offset) as *const u32) }
}

fn read_mmio_u64(base: *const u8, offset: usize) -> u64 {
    unsafe { read_volatile(base.add(offset) as *const u64) }
}

fn endpoint_dci(endpoint_addr: u8) -> u8 {
    let ep_num = endpoint_addr & 0x0F;
    if ep_num == 0 {
        1
    } else {
        (ep_num << 1) | if (endpoint_addr & 0x80) != 0 { 1 } else { 0 }
    }
}

fn log_endpoint_context(label: &str, ctx_ptr: *const u8) {
    let mut dw = [0u32; 8];
    for (idx, slot) in dw.iter_mut().enumerate() {
        *slot = unsafe { read_unaligned(ctx_ptr.add(idx * 4) as *const u32) };
    }

    let state = dw[0] & 0x7;
    let interval = (dw[0] >> 16) & 0xFF;
    let ep_type = (dw[1] >> 3) & 0x7;
    let max_burst = (dw[1] >> 8) & 0xFF;
    let max_packet_size = (dw[1] >> 16) & 0xFFFF;
    let dequeue_ptr = (((dw[3] as u64) << 32) | (dw[2] as u64)) & !0xFu64;
    let dcs = dw[2] & 0x1;
    let avg_trb_len = dw[4] & 0xFFFF;
    let max_esit_payload = (dw[4] >> 16) & 0xFFFF;

    crate::log!(
        "crabusb: xhci {} state={} type={} interval={} mps={} burst={} dcs={} tr_deq=0x{:X} avg_trb={} max_esit_payload={} raw=[{:08X} {:08X} {:08X} {:08X} {:08X} {:08X} {:08X} {:08X}]\n",
        label,
        state,
        ep_type,
        interval,
        max_packet_size,
        max_burst,
        dcs,
        dequeue_ptr,
        avg_trb_len,
        max_esit_payload,
        dw[0],
        dw[1],
        dw[2],
        dw[3],
        dw[4],
        dw[5],
        dw[6],
        dw[7]
    );
}

fn log_slot_context(ctx_ptr: *const u8) {
    let mut dw = [0u32; 8];
    for (idx, slot) in dw.iter_mut().enumerate() {
        *slot = unsafe { read_unaligned(ctx_ptr.add(idx * 4) as *const u32) };
    }
    let route = dw[0] & 0xFFFFF;
    let speed = (dw[0] >> 20) & 0xF;
    let context_entries = (dw[0] >> 27) & 0x1F;
    let root_port = (dw[1] >> 16) & 0xFF;
    let max_exit_latency = dw[1] & 0xFFFF;
    crate::log!(
        "crabusb: xhci slot route=0x{:X} speed={} entries={} root_port={} max_exit_latency={} raw=[{:08X} {:08X} {:08X} {:08X} {:08X} {:08X} {:08X} {:08X}]\n",
        route,
        speed,
        context_entries,
        root_port,
        max_exit_latency,
        dw[0],
        dw[1],
        dw[2],
        dw[3],
        dw[4],
        dw[5],
        dw[6],
        dw[7]
    );
}

async fn read_and_validate_csw(
    bulk_in: &mut EndpointBulkIn,
    cmd: &'static str,
    expected_tag: u32,
    bulk_in_ep: u8,
) -> Result<(), MassProbeError> {
    let mut csw = [0u8; 13];
    let mut csw_got = 0usize;
    for _ in 0..BOT_IO_RETRIES {
        let Some(result) =
            with_timeout_or_none(bulk_in.submit_and_wait(&mut csw), BOT_IO_TIMEOUT_MS).await
        else {
            log_bot_transport_debug(
                "csw-timeout",
                cmd,
                expected_tag,
                bulk_in_ep,
                1,
                csw.len(),
                csw.as_ptr(),
            );
            return Err(MassProbeError::Transport("csw-timeout"));
        };
        csw_got = result.map_err(|_| {
            log_bot_transport_debug(
                "csw-in",
                cmd,
                expected_tag,
                bulk_in_ep,
                1,
                csw.len(),
                csw.as_ptr(),
            );
            MassProbeError::Transport("csw-in")
        })?;
        if csw_got != 0 {
            break;
        }
    }
    if csw_got != csw.len() {
        return Err(MassProbeError::ShortData {
            cmd,
            got: csw_got,
            need: csw.len(),
        });
    }

    let sig = u32::from_le_bytes([csw[0], csw[1], csw[2], csw[3]]);
    let csw_tag = u32::from_le_bytes([csw[4], csw[5], csw[6], csw[7]]);
    let status = csw[12];
    if sig != 0x5342_5355 || csw_tag != expected_tag || status != 0 {
        return Err(MassProbeError::Csw {
            cmd,
            sig,
            tag: csw_tag,
            expected_tag,
            status,
        });
    }

    Ok(())
}

async fn bot_command_in(
    bulk_out: &mut EndpointBulkOut,
    bulk_in: &mut EndpointBulkIn,
    bulk_out_ep: u8,
    bulk_in_ep: u8,
    cmd: &'static str,
    lun: u8,
    cdb: &[u8],
    data: &mut [u8],
    tag: u32,
) -> Result<usize, MassProbeError> {
    let cbw = make_cbw(tag, data.len() as u32, 0x80, lun, cdb);
    let mut sent = 0usize;
    for _ in 0..BOT_IO_RETRIES {
        let Some(result) =
            with_timeout_or_none(bulk_out.submit_and_wait(&cbw), BOT_IO_TIMEOUT_MS).await
        else {
            log_bot_transport_debug(
                "cbw-timeout",
                cmd,
                tag,
                bulk_out_ep,
                2,
                cbw.len(),
                cbw.as_ptr(),
            );
            log_transport_debug("cbw-timeout");
            return Err(MassProbeError::Transport("cbw-timeout"));
        };
        sent = result.map_err(|_| {
            log_bot_transport_debug("cbw-out", cmd, tag, bulk_out_ep, 2, cbw.len(), cbw.as_ptr());
            log_transport_debug("cbw-out");
            MassProbeError::Transport("cbw-out")
        })?;
        if sent != 0 {
            break;
        }
    }
    if sent != cbw.len() {
        return Err(MassProbeError::ShortData {
            cmd,
            got: sent,
            need: cbw.len(),
        });
    }

    let mut got = 0usize;
    for _ in 0..BOT_IO_RETRIES {
        let Some(result) =
            with_timeout_or_none(bulk_in.submit_and_wait(data), BOT_IO_TIMEOUT_MS).await
        else {
            log_bot_transport_debug(
                "data-timeout",
                cmd,
                tag,
                bulk_in_ep,
                1,
                data.len(),
                data.as_ptr(),
            );
            log_transport_debug("data-timeout");
            return Err(MassProbeError::Transport("data-timeout"));
        };
        got = result.map_err(|_| {
            log_bot_transport_debug("data-in", cmd, tag, bulk_in_ep, 1, data.len(), data.as_ptr());
            log_transport_debug("data-in");
            MassProbeError::Transport("data-in")
        })?;
        if got != 0 {
            break;
        }
    }
    read_and_validate_csw(bulk_in, cmd, tag, bulk_in_ep).await?;
    Ok(got)
}

async fn bot_command_no_data(
    bulk_out: &mut EndpointBulkOut,
    bulk_in: &mut EndpointBulkIn,
    bulk_out_ep: u8,
    bulk_in_ep: u8,
    cmd: &'static str,
    lun: u8,
    cdb: &[u8],
    tag: u32,
) -> Result<(), MassProbeError> {
    let cbw = make_cbw(tag, 0, 0x00, lun, cdb);
    let mut sent = 0usize;
    for _ in 0..BOT_IO_RETRIES {
        let Some(result) =
            with_timeout_or_none(bulk_out.submit_and_wait(&cbw), BOT_IO_TIMEOUT_MS).await
        else {
            log_bot_transport_debug(
                "cbw-timeout",
                cmd,
                tag,
                bulk_out_ep,
                2,
                cbw.len(),
                cbw.as_ptr(),
            );
            log_transport_debug("cbw-timeout");
            return Err(MassProbeError::Transport("cbw-timeout"));
        };
        sent = result.map_err(|_| {
            log_bot_transport_debug("cbw-out", cmd, tag, bulk_out_ep, 2, cbw.len(), cbw.as_ptr());
            log_transport_debug("cbw-out");
            MassProbeError::Transport("cbw-out")
        })?;
        if sent != 0 {
            break;
        }
    }
    if sent != cbw.len() {
        return Err(MassProbeError::ShortData {
            cmd,
            got: sent,
            need: cbw.len(),
        });
    }

    read_and_validate_csw(bulk_in, cmd, tag, bulk_in_ep).await
}

async fn bot_command_out(
    bulk_out: &mut EndpointBulkOut,
    bulk_in: &mut EndpointBulkIn,
    bulk_out_ep: u8,
    bulk_in_ep: u8,
    cmd: &'static str,
    lun: u8,
    cdb: &[u8],
    data: &[u8],
    tag: u32,
) -> Result<(), MassProbeError> {
    let cbw = make_cbw(tag, data.len() as u32, 0x00, lun, cdb);
    let mut sent = 0usize;
    for _ in 0..BOT_IO_RETRIES {
        let Some(result) =
            with_timeout_or_none(bulk_out.submit_and_wait(&cbw), BOT_IO_TIMEOUT_MS).await
        else {
            log_bot_transport_debug(
                "cbw-timeout",
                cmd,
                tag,
                bulk_out_ep,
                2,
                cbw.len(),
                cbw.as_ptr(),
            );
            log_transport_debug("cbw-timeout");
            return Err(MassProbeError::Transport("cbw-timeout"));
        };
        sent = result.map_err(|_| {
            log_bot_transport_debug("cbw-out", cmd, tag, bulk_out_ep, 2, cbw.len(), cbw.as_ptr());
            log_transport_debug("cbw-out");
            MassProbeError::Transport("cbw-out")
        })?;
        if sent != 0 {
            break;
        }
    }
    if sent != cbw.len() {
        return Err(MassProbeError::ShortData {
            cmd,
            got: sent,
            need: cbw.len(),
        });
    }

    let mut data_sent = 0usize;
    for _ in 0..BOT_IO_RETRIES {
        let Some(result) =
            with_timeout_or_none(bulk_out.submit_and_wait(data), BOT_IO_TIMEOUT_MS).await
        else {
            log_bot_transport_debug(
                "data-timeout",
                cmd,
                tag,
                bulk_out_ep,
                2,
                data.len(),
                data.as_ptr(),
            );
            log_transport_debug("data-timeout");
            return Err(MassProbeError::Transport("data-timeout"));
        };
        data_sent = result.map_err(|_| {
            log_bot_transport_debug(
                "data-out",
                cmd,
                tag,
                bulk_out_ep,
                2,
                data.len(),
                data.as_ptr(),
            );
            log_transport_debug("data-out");
            MassProbeError::Transport("data-out")
        })?;
        if data_sent != 0 {
            break;
        }
    }
    if data_sent != data.len() {
        return Err(MassProbeError::ShortData {
            cmd,
            got: data_sent,
            need: data.len(),
        });
    }

    read_and_validate_csw(bulk_in, cmd, tag, bulk_in_ep).await
}

async fn uas_send_command(
    command_out: &mut EndpointBulkOut,
    cmd: &'static str,
    cdb: &[u8],
    tag: u16,
) -> Result<(), MassProbeError> {
    let iu = make_uas_command_iu(tag, cdb);
    if uas_probe_logs_enabled() {
        log_uas_iu("command-iu", cmd, tag, &iu);
    }
    let sent = with_timeout_or_none(command_out.submit_and_wait(&iu), UAS_IO_TIMEOUT_MS)
        .await
        .ok_or_else(|| {
            log_uas_debug("command-timeout", cmd, tag);
            MassProbeError::Transport("uas-command-timeout")
        })?
        .map_err(|_| MassProbeError::Transport("uas-command-out"))?;
    log_uas_debug("command-complete", cmd, tag);
    if sent != iu.len() {
        return Err(MassProbeError::ShortData {
            cmd,
            got: sent,
            need: iu.len(),
        });
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

async fn uas_read_iu(
    status_in: &mut EndpointBulkIn,
    cmd: &'static str,
    stream_id: u16,
    buf: &mut [u8],
) -> Result<usize, MassProbeError> {
    let got = with_timeout_or_none(
        status_in.submit_on_stream_and_wait(stream_id, buf),
        UAS_IO_TIMEOUT_MS,
    )
    .await
    .ok_or_else(|| {
        log_transport_debug("uas-status-timeout");
        MassProbeError::Transport("uas-status-timeout")
    })?
    .map_err(|_| MassProbeError::Transport("uas-status-in"))?;
    if got < 4 {
        return Err(MassProbeError::ShortData { cmd, got, need: 4 });
    }
    Ok(got)
}

async fn uas_expect_ready(
    status_in: &mut EndpointBulkIn,
    cmd: &'static str,
    expected_tag: u16,
    expected_iu: u8,
) -> Result<(), MassProbeError> {
    let mut iu = [0u8; 16];
    let got = uas_read_iu(status_in, cmd, expected_tag, &mut iu).await?;
    let iu = &iu[..got.min(iu.len())];
    let iu_id = iu[0];
    let tag = parse_uas_tag(iu).unwrap_or(0);
    if iu_id != expected_iu || tag != expected_tag {
        return Err(MassProbeError::Csw {
            cmd,
            sig: u32::from(iu_id),
            tag: u32::from(tag),
            expected_tag: u32::from(expected_tag),
            status: 0xFF,
        });
    }
    Ok(())
}

async fn uas_read_status(
    status_in: &mut EndpointBulkIn,
    cmd: &'static str,
    expected_tag: u16,
) -> Result<(), MassProbeError> {
    let mut status = [0u8; 96];
    let got = uas_read_iu(status_in, cmd, expected_tag, &mut status).await?;
    log_uas_iu("status-iu", cmd, expected_tag, &status[..got.min(status.len())]);
    validate_uas_status(cmd, &status[..got.min(status.len())], expected_tag)
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
                            return Err(MassProbeError::Csw {
                                cmd,
                                sig: u32::from(ready_id),
                                tag: u32::from(ready_tag),
                                expected_tag: u32::from(tag),
                                status: 0xFF,
                            });
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
                return Err(MassProbeError::ShortData {
                    cmd,
                    got: ready_got,
                    need: 4,
                });
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
                return Err(MassProbeError::Csw {
                    cmd,
                    sig: u32::from(ready_id),
                    tag: u32::from(ready_tag),
                    expected_tag: u32::from(tag),
                    status: 0xFF,
                });
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
    let data_handle = data_out
        .submit_on_stream(tag, data)
        .map_err(|_| MassProbeError::Transport("uas-data-submit"))?;
    log_uas_debug("data-out-submit", cmd, tag);
    uas_send_command(command_out, cmd, cdb, tag).await?;

    let ready_got = with_timeout_or_none(ready_handle, UAS_IO_TIMEOUT_MS)
        .await
        .ok_or_else(|| {
            log_uas_debug("ready-timeout", cmd, tag);
            MassProbeError::Transport("uas-status-timeout")
        })?
        .map_err(|_| MassProbeError::Transport("uas-status-in"))?
        .transfer_len;
    if ready_got < 4 {
        return Err(MassProbeError::ShortData {
            cmd,
            got: ready_got,
            need: 4,
        });
    }
    let ready = &ready_iu[..ready_got.min(ready_iu.len())];
    log_uas_iu("ready-iu", cmd, tag, ready);
    let ready_id = ready[0];
    let ready_tag = parse_uas_tag(ready).unwrap_or(0);
    if ready_id == UAS_IU_STATUS && ready_tag == tag {
        validate_uas_status(cmd, ready, tag)?;
        log_uas_debug("status-before-data", cmd, tag);
        let sent = with_timeout_or_none(data_handle, UAS_IO_TIMEOUT_MS)
            .await
            .ok_or_else(|| {
                log_uas_debug("data-out-timeout-after-status", cmd, tag);
                MassProbeError::Transport("uas-data-timeout")
            })?
            .map_err(|_| MassProbeError::Transport("uas-data-out"))?
            .transfer_len;
        log_uas_debug("data-out-complete-after-status", cmd, tag);
        if sent != data.len() {
            return Err(MassProbeError::ShortData {
                cmd,
                got: sent,
                need: data.len(),
            });
        }
        return Ok(());
    }
    if ready_id != UAS_IU_WRITE_READY || ready_tag != tag {
        return Err(MassProbeError::Csw {
            cmd,
            sig: u32::from(ready_id),
            tag: u32::from(ready_tag),
            expected_tag: u32::from(tag),
            status: 0xFF,
        });
    }

    let sent = with_timeout_or_none(data_handle, UAS_IO_TIMEOUT_MS)
        .await
        .ok_or_else(|| {
            log_uas_debug("data-out-timeout", cmd, tag);
            MassProbeError::Transport("uas-data-timeout")
        })?
        .map_err(|_| MassProbeError::Transport("uas-data-out"))?
        .transfer_len;
    log_uas_debug("data-out-complete", cmd, tag);
    if sent != data.len() {
        return Err(MassProbeError::ShortData {
            cmd,
            got: sent,
            need: data.len(),
        });
    }
    uas_read_status(status_in, cmd, tag).await
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

pub(crate) async fn bot_recovery(
    device: &mut crab_usb::Device,
    interface_number: u8,
    bulk_out_ep: u8,
    bulk_in_ep: u8,
) -> Result<(), MassProbeError> {
    crate::log!(
        "crabusb: mass recovery if#{} bulk_out=0x{:02X} bulk_in=0x{:02X} step=reset\n",
        interface_number,
        bulk_out_ep,
        bulk_in_ep
    );
    device
        .control_out(
            ControlSetup {
                request_type: RequestType::Class,
                recipient: Recipient::Interface,
                request: Request::Other(0xFF),
                value: 0,
                index: interface_number as u16,
            },
            &[],
        )
        .await
        .map_err(|_| MassProbeError::Transport("bot-reset"))?;

    crate::log!(
        "crabusb: mass recovery if#{} step=clear-halt-out ep=0x{:02X}\n",
        interface_number,
        bulk_out_ep
    );
    match control_out_with_timeout(
        device,
        ControlSetup {
            request_type: RequestType::Standard,
            recipient: Recipient::Endpoint,
            request: Request::ClearFeature,
            value: 0,
            index: bulk_out_ep as u16,
        },
        &[],
        BOT_IO_TIMEOUT_MS,
    )
    .await
    {
        Some(Ok(_)) => {}
        Some(Err(err)) => {
            crate::log!(
                "crabusb: mass recovery if#{} clear-halt-out ep=0x{:02X} ignored err={:?}\n",
                interface_number,
                bulk_out_ep,
                err
            );
        }
        None => {
            crate::log!(
                "crabusb: mass recovery if#{} clear-halt-out ep=0x{:02X} ignored timeout\n",
                interface_number,
                bulk_out_ep
            );
        }
    }

    crate::log!(
        "crabusb: mass recovery if#{} step=clear-halt-in ep=0x{:02X}\n",
        interface_number,
        bulk_in_ep
    );
    match control_out_with_timeout(
        device,
        ControlSetup {
            request_type: RequestType::Standard,
            recipient: Recipient::Endpoint,
            request: Request::ClearFeature,
            value: 0,
            index: bulk_in_ep as u16,
        },
        &[],
        BOT_IO_TIMEOUT_MS,
    )
    .await
    {
        Some(Ok(_)) => {}
        Some(Err(err)) => {
            crate::log!(
                "crabusb: mass recovery if#{} clear-halt-in ep=0x{:02X} ignored err={:?}\n",
                interface_number,
                bulk_in_ep,
                err
            );
        }
        None => {
            crate::log!(
                "crabusb: mass recovery if#{} clear-halt-in ep=0x{:02X} ignored timeout\n",
                interface_number,
                bulk_in_ep
            );
        }
    }

    crate::log!("crabusb: mass recovery if#{} step=done\n", interface_number);
    Timer::after(EmbassyDuration::from_millis(BOT_RECOVERY_SETTLE_MS)).await;

    Ok(())
}

fn decode_ascii_field(field: &[u8]) -> String {
    let mut out = String::new();
    for &b in field {
        if (0x20..=0x7E).contains(&b) {
            out.push(b as char);
        } else {
            out.push(' ');
        }
    }
    String::from(out.trim())
}

fn log_request_sense(data: &[u8]) {
    if data.len() < 14 {
        crate::log!("crabusb: mass request-sense short len={}\n", data.len());
        return;
    }

    let response_code = data[0] & 0x7F;
    let sense_key = data[2] & 0x0F;
    let asc = data[12];
    let ascq = data[13];
    crate::log!(
        "crabusb: mass request-sense rc=0x{:02X} key=0x{:02X} asc=0x{:02X} ascq=0x{:02X}\n",
        response_code,
        sense_key,
        asc,
        ascq
    );
}

async fn request_sense(
    bulk_out: &mut EndpointBulkOut,
    bulk_in: &mut EndpointBulkIn,
    bulk_out_ep: u8,
    bulk_in_ep: u8,
    lun: u8,
    tag: u32,
) -> Result<usize, MassProbeError> {
    let mut sense = [0u8; 18];
    let request_sense_cdb = [0x03, 0, 0, 0, sense.len() as u8, 0];
    let got = bot_command_in(
        bulk_out,
        bulk_in,
        bulk_out_ep,
        bulk_in_ep,
        "request-sense",
        lun,
        &request_sense_cdb,
        &mut sense,
        tag,
    )
    .await?;
    log_request_sense(&sense[..got.min(sense.len())]);
    Ok(got)
}

pub(crate) async fn request_sense_fixed(
    bulk_out: &mut EndpointBulkOut,
    bulk_in: &mut EndpointBulkIn,
    bulk_out_ep: u8,
    bulk_in_ep: u8,
    tag: u32,
) -> Option<scsi::SenseFixed> {
    let mut sense = [0u8; 18];
    let cdb = scsi::cdb_request_sense(18);
    let got = bot_command_in(
        bulk_out,
        bulk_in,
        bulk_out_ep,
        bulk_in_ep,
        "request-sense",
        0,
        &cdb,
        &mut sense,
        tag,
    )
    .await
    .ok()?;

    scsi::parse_request_sense_fixed(&sense[..got.min(sense.len())])
}

async fn test_unit_ready_with_sense(
    bulk_out: &mut EndpointBulkOut,
    bulk_in: &mut EndpointBulkIn,
    bulk_out_ep: u8,
    bulk_in_ep: u8,
    lun: u8,
) -> Result<(), MassProbeError> {
    let tur_cdb = [0x00, 0, 0, 0, 0, 0];
    for attempt in 0..3u32 {
        match bot_command_no_data(
            bulk_out,
            bulk_in,
            bulk_out_ep,
            bulk_in_ep,
            "test-unit-ready",
            lun,
            &tur_cdb,
            0x544F_5200 + attempt,
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(err) => {
                crate::log!(
                    "crabusb: mass test-unit-ready attempt {} failed: {:?}\n",
                    attempt + 1,
                    err
                );
                let _ = request_sense(
                    bulk_out,
                    bulk_in,
                    bulk_out_ep,
                    bulk_in_ep,
                    lun,
                    0x544F_5300 + attempt,
                )
                .await;
                Timer::after(EmbassyDuration::from_millis(20)).await;
            }
        }
    }
    Ok(())
}

pub(crate) async fn keepalive_mass_bot(
    bulk_out: &mut EndpointBulkOut,
    bulk_in: &mut EndpointBulkIn,
    bulk_out_ep: u8,
    bulk_in_ep: u8,
    lun: u8,
) -> Result<(), MassProbeError> {
    test_unit_ready_with_sense(bulk_out, bulk_in, bulk_out_ep, bulk_in_ep, lun).await
}

pub(crate) async fn read_blocks_bot(
    bulk_out: &mut EndpointBulkOut,
    bulk_in: &mut EndpointBulkIn,
    bulk_out_ep: u8,
    bulk_in_ep: u8,
    lba: u32,
    blocks: u16,
    out: &mut [u8],
    tag: u32,
) -> Result<(), MassProbeError> {
    let cdb = scsi::cdb_read_10(lba, blocks);
    let got =
        bot_command_in(bulk_out, bulk_in, bulk_out_ep, bulk_in_ep, "read-10", 0, &cdb, out, tag)
            .await?;
    if got < out.len() {
        return Err(MassProbeError::ShortData {
            cmd: "read-10",
            got,
            need: out.len(),
        });
    }
    Ok(())
}

pub(crate) async fn write_blocks_bot(
    bulk_out: &mut EndpointBulkOut,
    bulk_in: &mut EndpointBulkIn,
    bulk_out_ep: u8,
    bulk_in_ep: u8,
    lba: u32,
    blocks: u16,
    data: &[u8],
    tag: u32,
) -> Result<(), MassProbeError> {
    let cdb = scsi::cdb_write_10(lba, blocks);
    bot_command_out(bulk_out, bulk_in, bulk_out_ep, bulk_in_ep, "write-10", 0, &cdb, data, tag)
        .await
}

pub(crate) async fn synchronize_cache_bot(
    bulk_out: &mut EndpointBulkOut,
    bulk_in: &mut EndpointBulkIn,
    bulk_out_ep: u8,
    bulk_in_ep: u8,
    tag: u32,
) -> Result<(), MassProbeError> {
    let cdb = scsi::cdb_synchronize_cache_10();
    bot_command_no_data(bulk_out, bulk_in, bulk_out_ep, bulk_in_ep, "sync-cache-10", 0, &cdb, tag)
        .await
}

pub(crate) async fn request_sense_fixed_uas_skhynix(
    command_out: &mut EndpointBulkOut,
    status_in: &mut EndpointBulkIn,
    data_in: &mut EndpointBulkIn,
    tag: u32,
) -> Option<scsi::SenseFixed> {
    let mut sense = [0u8; 18];
    let cdb = scsi::cdb_request_sense(18);
    let got =
        uas_command_in(command_out, status_in, data_in, "request-sense", &cdb, &mut sense, tag)
            .await
            .ok()?;

    scsi::parse_request_sense_fixed(&sense[..got.min(sense.len())])
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
        return Err(MassProbeError::ShortData {
            cmd: "read-10",
            got,
            need: out.len(),
        });
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
        return Err(MassProbeError::ShortData {
            cmd: "read-buffer-echo",
            got,
            need: pattern.len(),
        });
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

    let block_size = info.block_size as usize;
    if block_size == 0 || block_size > 1024 * 1024 {
        return Err(MassProbeError::ShortData {
            cmd: "uas-exercise-block-size",
            got: block_size,
            need: 1,
        });
    }

    let mut first_block = vec![0u8; block_size];
    read_blocks_uas_skhynix(
        command_out,
        status_in,
        data_in,
        0,
        1,
        first_block.as_mut_slice(),
        0x5541_EC10,
    )
    .await?;
    let checksum = first_block
        .iter()
        .fold(0u32, |acc, &byte| acc.wrapping_mul(33).wrapping_add(u32::from(byte)));
    crate::log!(
        "crabusb: mass uas-skhynix exercise read-lba0 bytes={} checksum=0x{:08X}\n",
        first_block.len(),
        checksum
    );

    crate::log!("crabusb: mass uas-skhynix exercise echo-buffer skipped reason=read-bringup\n");

    Ok(info)
}

async fn read_capacity_16(
    bulk_out: &mut EndpointBulkOut,
    bulk_in: &mut EndpointBulkIn,
    bulk_out_ep: u8,
    bulk_in_ep: u8,
    lun: u8,
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
    let got = bot_command_in(
        bulk_out,
        bulk_in,
        bulk_out_ep,
        bulk_in_ep,
        "read-capacity16",
        lun,
        &read_capacity_cdb,
        &mut read_capacity,
        0x544F_4E53,
    )
    .await?;
    if got < 12 {
        return Err(MassProbeError::ShortData {
            cmd: "read-capacity16",
            got,
            need: 12,
        });
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

async fn read_format_capacities(
    bulk_out: &mut EndpointBulkOut,
    bulk_in: &mut EndpointBulkIn,
    bulk_out_ep: u8,
    bulk_in_ep: u8,
    lun: u8,
) -> Result<(u64, u32), MassProbeError> {
    let mut buf = [0u8; 64];
    let alloc_len = buf.len() as u16;
    let cdb = [
        0x23,
        0,
        0,
        0,
        (alloc_len >> 8) as u8,
        alloc_len as u8,
        0,
        0,
        0,
        0,
    ];
    let got = bot_command_in(
        bulk_out,
        bulk_in,
        bulk_out_ep,
        bulk_in_ep,
        "read-format-capacities",
        lun,
        &cdb,
        &mut buf,
        0x544F_4E54,
    )
    .await?;
    if got < 12 {
        return Err(MassProbeError::ShortData {
            cmd: "read-format-capacities",
            got,
            need: 12,
        });
    }
    let descriptor_code = buf[8 + 4] & 0x03;
    let block_count = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]) as u64;
    let block_size = u32::from_be_bytes([0, buf[13], buf[14], buf[15]]);
    crate::log!(
        "crabusb: mass read-format-capacities code={} bs={} blocks={}\n",
        descriptor_code,
        block_size,
        block_count
    );
    Ok((block_count, block_size))
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
        return Err(MassProbeError::ShortData {
            cmd: "read-capacity16",
            got,
            need: 12,
        });
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
    let max_lun = 0u8;
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
        return Err(MassProbeError::ShortData {
            cmd: "inquiry",
            got: inquiry_read,
            need: 32,
        });
    }

    let removable = (inquiry[1] & 0x80) != 0;
    crate::log!(
        "crabusb: mass uas-skhynix inquiry removable={} pdt=0x{:02X}\n",
        removable,
        inquiry[0] & 0x1F
    );
    let _ = keepalive_mass_uas_skhynix(command_out, status_in, 0x5541_5200).await;

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
                return Err(MassProbeError::ShortData {
                    cmd: "read-capacity10",
                    got: read_capacity_read,
                    need: read_capacity.len(),
                });
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
            crate::log!(
                "crabusb: mass uas-skhynix read-capacity10 failed: {:?}; trying capacity16\n",
                err
            );
            let _ =
                request_sense_fixed_uas_skhynix(command_out, status_in, data_in, 0x5541_4E55).await;
            read_capacity_16_uas_skhynix(command_out, status_in, data_in).await?
        }
    };
    if block_size == 0 {
        return Err(MassProbeError::ShortData {
            cmd: "read-capacity10",
            got: 0,
            need: 1,
        });
    }

    let vendor = decode_ascii_field(&inquiry[8..16]);
    let product = decode_ascii_field(&inquiry[16..32]);

    Ok(MassProbeInfo {
        max_lun,
        block_size,
        block_count,
        vendor,
        product,
    })
}

pub(crate) async fn probe_mass_bot(
    device: &mut crab_usb::Device,
    bulk_out: &mut EndpointBulkOut,
    bulk_in: &mut EndpointBulkIn,
    interface_number: u8,
    bulk_out_ep: u8,
    bulk_in_ep: u8,
) -> Result<MassProbeInfo, MassProbeError> {
    let mut max_lun_buf = [0u8; 1];
    let max_lun = match device
        .control_in(
            ControlSetup {
                request_type: RequestType::Class,
                recipient: Recipient::Interface,
                request: Request::Other(0xFE),
                value: 0,
                index: interface_number as u16,
            },
            &mut max_lun_buf,
        )
        .await
    {
        Ok(read) if read >= 1 => max_lun_buf[0],
        Ok(_) => 0,
        Err(TransferError::Stall) => 0,
        Err(_) => return Err(MassProbeError::Transport("get-max-lun")),
    };

    let lun = 0u8;
    let mut inquiry = [0u8; 36];
    let inquiry_cdb = [0x12, 0, 0, 0, inquiry.len() as u8, 0];
    let inquiry_read = match bot_command_in(
        bulk_out,
        bulk_in,
        bulk_out_ep,
        bulk_in_ep,
        "inquiry",
        lun,
        &inquiry_cdb,
        &mut inquiry,
        0x544F_4E51,
    )
    .await
    {
        Ok(read) => read,
        Err(first_err) => {
            crate::log!(
                "crabusb: mass inquiry initial attempt failed: {:?}; applying BOT recovery\n",
                first_err
            );
            bot_recovery(device, interface_number, bulk_out_ep, bulk_in_ep).await?;
            bot_command_in(
                bulk_out,
                bulk_in,
                bulk_out_ep,
                bulk_in_ep,
                "inquiry",
                lun,
                &inquiry_cdb,
                &mut inquiry,
                0x544F_4E51,
            )
            .await?
        }
    };
    if inquiry_read < 32 {
        return Err(MassProbeError::ShortData {
            cmd: "inquiry",
            got: inquiry_read,
            need: 32,
        });
    }

    let removable = (inquiry[1] & 0x80) != 0;
    crate::log!("crabusb: mass inquiry removable={} pdt=0x{:02X}\n", removable, inquiry[0] & 0x1F);
    let _ = test_unit_ready_with_sense(bulk_out, bulk_in, bulk_out_ep, bulk_in_ep, lun).await;

    let mut read_capacity = [0u8; 8];
    let read_capacity_cdb = [0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let (block_count, block_size) = match bot_command_in(
        bulk_out,
        bulk_in,
        bulk_out_ep,
        bulk_in_ep,
        "read-capacity10",
        lun,
        &read_capacity_cdb,
        &mut read_capacity,
        0x544F_4E52,
    )
    .await
    {
        Ok(read_capacity_read) => {
            if read_capacity_read < read_capacity.len() {
                return Err(MassProbeError::ShortData {
                    cmd: "read-capacity10",
                    got: read_capacity_read,
                    need: read_capacity.len(),
                });
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
            crate::log!(
                "crabusb: mass read-capacity10 failed: {:?}; trying sense/capacity fallbacks\n",
                err
            );
            let _ =
                request_sense(bulk_out, bulk_in, bulk_out_ep, bulk_in_ep, lun, 0x544F_4E55).await;
            match read_capacity_16(bulk_out, bulk_in, bulk_out_ep, bulk_in_ep, lun).await {
                Ok((blocks, bs)) => {
                    crate::log!(
                        "crabusb: mass read-capacity16 fallback bs={} blocks={}\n",
                        bs,
                        blocks
                    );
                    (blocks, bs)
                }
                Err(rc16_err) => {
                    crate::log!(
                        "crabusb: mass read-capacity16 failed: {:?}; trying read-format-capacities\n",
                        rc16_err
                    );
                    let _ =
                        request_sense(bulk_out, bulk_in, bulk_out_ep, bulk_in_ep, lun, 0x544F_4E56)
                            .await;
                    let (blocks, bs) =
                        read_format_capacities(bulk_out, bulk_in, bulk_out_ep, bulk_in_ep, lun)
                            .await?;
                    (blocks, bs)
                }
            }
        }
    };
    if block_size == 0 {
        return Err(MassProbeError::ShortData {
            cmd: "read-capacity10",
            got: 0,
            need: 1,
        });
    }

    let vendor = decode_ascii_field(&inquiry[8..16]);
    let product = decode_ascii_field(&inquiry[16..32]);

    Ok(MassProbeInfo {
        max_lun,
        block_size,
        block_count,
        vendor,
        product,
    })
}

struct UsbMassGeometryPlaceholderDevice {
    block_size: u32,
    block_count: u64,
}

impl block::BlockDevice for UsbMassGeometryPlaceholderDevice {
    fn block_size_bytes(&self) -> u32 {
        self.block_size
    }

    fn block_count(&self) -> u64 {
        self.block_count
    }

    fn read_blocks<'a>(
        &'a mut self,
        _lba: u64,
        _blocks: usize,
    ) -> block::BoxFuture<'a, block::Result<Vec<u8>>> {
        Box::pin(async { Err(block::Error::NotSupported) })
    }
}

pub(crate) fn register_mass_geometry_placeholder(
    vendor_id: u16,
    product_id: u16,
    block_size: u32,
    block_count: u64,
) -> block::DeviceHandle {
    let label = alloc::format!("usbms-{:04X}:{:04X}", vendor_id, product_id);
    let desc = block::DeviceDescriptor::new(block::DeviceKind::Unknown).with_label(label);
    block::register_device(
        desc,
        UsbMassGeometryPlaceholderDevice {
            block_size: block_size.max(1),
            block_count: block_count.max(1),
        },
    )
}
