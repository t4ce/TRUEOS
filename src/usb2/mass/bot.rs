use crab_usb::{EndpointBulkIn, EndpointBulkOut, usb_if};
use embassy_time::{Duration as EmbassyDuration, Timer};
use usb_if::host::ControlSetup;
use usb_if::transfer::{Recipient, Request, RequestType};

use super::super::scsi;
use super::{
    BOT_IO_RETRIES, BOT_IO_TIMEOUT_MS, BOT_RECOVERY_SETTLE_MS, MassProbeError, MassProbeInfo,
    USB_CLASS_MASS_STORAGE, USB_PROTO_BULK_ONLY, USB_SUBCLASS_SCSI, control_out_with_timeout,
    decode_ascii_field, log_transport_debug, with_timeout_or_none,
};

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
    crate::log_trace!(
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

fn endpoint_dci(endpoint_addr: u8) -> u8 {
    let ep_num = endpoint_addr & 0x0F;
    if ep_num == 0 {
        1
    } else {
        (ep_num << 1) | if (endpoint_addr & 0x80) != 0 { 1 } else { 0 }
    }
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
        return Err(MassProbeError::ShortData);
    }

    let sig = u32::from_le_bytes([csw[0], csw[1], csw[2], csw[3]]);
    let csw_tag = u32::from_le_bytes([csw[4], csw[5], csw[6], csw[7]]);
    let status = csw[12];
    if sig != 0x5342_5355 || csw_tag != expected_tag || status != 0 {
        return Err(MassProbeError::Csw);
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
        return Err(MassProbeError::ShortData);
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
        return Err(MassProbeError::ShortData);
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
        return Err(MassProbeError::ShortData);
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
        return Err(MassProbeError::ShortData);
    }

    read_and_validate_csw(bulk_in, cmd, tag, bulk_in_ep).await
}

pub(crate) async fn bot_recovery(
    device: &mut crab_usb::Device,
    interface_number: u8,
    bulk_out_ep: u8,
    bulk_in_ep: u8,
) -> Result<(), MassProbeError> {
    crate::log_trace!(
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

    crate::log_trace!(
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
            crate::log_trace!(
                "crabusb: mass recovery if#{} clear-halt-out ep=0x{:02X} ignored err={:?}\n",
                interface_number,
                bulk_out_ep,
                err
            );
        }
        None => {
            crate::log_trace!(
                "crabusb: mass recovery if#{} clear-halt-out ep=0x{:02X} ignored timeout\n",
                interface_number,
                bulk_out_ep
            );
        }
    }

    crate::log_trace!(
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
            crate::log_trace!(
                "crabusb: mass recovery if#{} clear-halt-in ep=0x{:02X} ignored err={:?}\n",
                interface_number,
                bulk_in_ep,
                err
            );
        }
        None => {
            crate::log_trace!(
                "crabusb: mass recovery if#{} clear-halt-in ep=0x{:02X} ignored timeout\n",
                interface_number,
                bulk_in_ep
            );
        }
    }

    crate::log_trace!("crabusb: mass recovery if#{} step=done\n", interface_number);
    Timer::after(EmbassyDuration::from_millis(BOT_RECOVERY_SETTLE_MS)).await;

    Ok(())
}

fn log_request_sense(data: &[u8]) {
    if data.len() < 14 {
        crate::log_trace!("crabusb: mass request-sense short len={}\n", data.len());
        return;
    }

    let response_code = data[0] & 0x7F;
    let sense_key = data[2] & 0x0F;
    let asc = data[12];
    let ascq = data[13];
    crate::log_trace!(
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
                crate::log_trace!(
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
        return Err(MassProbeError::ShortData);
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
        return Err(MassProbeError::ShortData);
    }
    let descriptor_code = buf[8 + 4] & 0x03;
    let block_count = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]) as u64;
    let block_size = u32::from_be_bytes([0, buf[13], buf[14], buf[15]]);
    crate::log_trace!(
        "crabusb: mass read-format-capacities code={} bs={} blocks={}\n",
        descriptor_code,
        block_size,
        block_count
    );
    Ok((block_count, block_size))
}

pub(crate) async fn probe_mass_bot(
    device: &mut crab_usb::Device,
    bulk_out: &mut EndpointBulkOut,
    bulk_in: &mut EndpointBulkIn,
    interface_number: u8,
    bulk_out_ep: u8,
    bulk_in_ep: u8,
) -> Result<MassProbeInfo, MassProbeError> {
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
            crate::log_trace!(
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
        return Err(MassProbeError::ShortData);
    }

    let removable = (inquiry[1] & 0x80) != 0;
    crate::log_trace!("crabusb: mass inquiry removable={} pdt=0x{:02X}\n", removable, inquiry[0] & 0x1F);
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
            crate::log_trace!(
                "crabusb: mass read-capacity10 failed: {:?}; trying sense/capacity fallbacks\n",
                err
            );
            let _ =
                request_sense(bulk_out, bulk_in, bulk_out_ep, bulk_in_ep, lun, 0x544F_4E55).await;
            match read_capacity_16(bulk_out, bulk_in, bulk_out_ep, bulk_in_ep, lun).await {
                Ok((blocks, bs)) => {
                    crate::log_trace!(
                        "crabusb: mass read-capacity16 fallback bs={} blocks={}\n",
                        bs,
                        blocks
                    );
                    (blocks, bs)
                }
                Err(rc16_err) => {
                    crate::log_trace!(
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
