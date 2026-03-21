use alloc::{boxed::Box, string::String, vec::Vec};
use core::{
    future::Future,
    ptr::{read_unaligned, read_volatile},
    task::Poll,
};
use crab_usb::{EndpointBulkIn, EndpointBulkOut, err::TransferError, usb_if};
use embassy_time::{Duration as EmbassyDuration, Timer};
use usb_if::host::ControlSetup;
use usb_if::transfer::{Recipient, Request, RequestType};

use crate::disc::block;

const USB_CLASS_MASS_STORAGE: u8 = 0x08;
const USB_SUBCLASS_SCSI: u8 = 0x06;
const USB_PROTO_BULK_ONLY: u8 = 0x50;
const BOT_IO_RETRIES: usize = 8;
const BOT_IO_TIMEOUT_MS: u64 = 500;
const BOT_RECOVERY_SETTLE_MS: u64 = 25;

#[derive(Copy, Clone, Debug)]
pub(crate) struct MassProbeConcept {
    pub name: &'static str,
    pub settle_after_claim_ms: u64,
    pub settle_after_open_ms: u64,
    pub pre_bot_reset: bool,
    pub pre_reset_bulk_out: bool,
    pub pre_reset_bulk_in: bool,
    pub reset_halted_bulk_in: bool,
    pub recover_on_first_failure: bool,
    pub reset_bulk_in_on_data_failure: bool,
}

pub(crate) const MASS_PROBE_CONCEPTS: [MassProbeConcept; 10] = [
    MassProbeConcept {
        name: "fresh-plain",
        settle_after_claim_ms: 0,
        settle_after_open_ms: 0,
        pre_bot_reset: false,
        pre_reset_bulk_out: false,
        pre_reset_bulk_in: false,
        reset_halted_bulk_in: false,
        recover_on_first_failure: false,
        reset_bulk_in_on_data_failure: false,
    },
    MassProbeConcept {
        name: "fresh-settle-10ms",
        settle_after_claim_ms: 10,
        settle_after_open_ms: 10,
        pre_bot_reset: false,
        pre_reset_bulk_out: false,
        pre_reset_bulk_in: false,
        reset_halted_bulk_in: false,
        recover_on_first_failure: false,
        reset_bulk_in_on_data_failure: false,
    },
    MassProbeConcept {
        name: "bot-reset-first",
        settle_after_claim_ms: 0,
        settle_after_open_ms: 10,
        pre_bot_reset: true,
        pre_reset_bulk_out: false,
        pre_reset_bulk_in: false,
        reset_halted_bulk_in: false,
        recover_on_first_failure: false,
        reset_bulk_in_on_data_failure: false,
    },
    MassProbeConcept {
        name: "repair-halted-in",
        settle_after_claim_ms: 0,
        settle_after_open_ms: 0,
        pre_bot_reset: false,
        pre_reset_bulk_out: false,
        pre_reset_bulk_in: false,
        reset_halted_bulk_in: true,
        recover_on_first_failure: false,
        reset_bulk_in_on_data_failure: false,
    },
    MassProbeConcept {
        name: "repair-both-preflight",
        settle_after_claim_ms: 0,
        settle_after_open_ms: 10,
        pre_bot_reset: false,
        pre_reset_bulk_out: true,
        pre_reset_bulk_in: true,
        reset_halted_bulk_in: false,
        recover_on_first_failure: false,
        reset_bulk_in_on_data_failure: false,
    },
    MassProbeConcept {
        name: "plain-plus-recovery",
        settle_after_claim_ms: 0,
        settle_after_open_ms: 0,
        pre_bot_reset: false,
        pre_reset_bulk_out: false,
        pre_reset_bulk_in: false,
        reset_halted_bulk_in: false,
        recover_on_first_failure: true,
        reset_bulk_in_on_data_failure: false,
    },
    MassProbeConcept {
        name: "repair-in-plus-recovery",
        settle_after_claim_ms: 0,
        settle_after_open_ms: 0,
        pre_bot_reset: false,
        pre_reset_bulk_out: false,
        pre_reset_bulk_in: false,
        reset_halted_bulk_in: true,
        recover_on_first_failure: true,
        reset_bulk_in_on_data_failure: true,
    },
    MassProbeConcept {
        name: "bot-reset-plus-recovery",
        settle_after_claim_ms: 10,
        settle_after_open_ms: 10,
        pre_bot_reset: true,
        pre_reset_bulk_out: false,
        pre_reset_bulk_in: false,
        reset_halted_bulk_in: true,
        recover_on_first_failure: true,
        reset_bulk_in_on_data_failure: true,
    },
    MassProbeConcept {
        name: "repair-both-plus-recovery",
        settle_after_claim_ms: 10,
        settle_after_open_ms: 10,
        pre_bot_reset: false,
        pre_reset_bulk_out: true,
        pre_reset_bulk_in: true,
        reset_halted_bulk_in: true,
        recover_on_first_failure: true,
        reset_bulk_in_on_data_failure: true,
    },
    MassProbeConcept {
        name: "slow-path",
        settle_after_claim_ms: 25,
        settle_after_open_ms: 25,
        pre_bot_reset: true,
        pre_reset_bulk_out: true,
        pre_reset_bulk_in: true,
        reset_halted_bulk_in: true,
        recover_on_first_failure: true,
        reset_bulk_in_on_data_failure: true,
    },
];

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

                let (bulk_in_addr, bulk_in_mps) = bulk_in?;
                let (bulk_out_addr, bulk_out_mps) = bulk_out?;

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
        "crabusb: mass debug stage={} last_submit[dci={} dir={} len={} ptr=0x{:X}] last_event[slot={} ep={} cc={} residual={} ptr=0x{:X}]\n",
        stage,
        submit.dci,
        submit.direction,
        submit.len,
        submit.ptr,
        event.slot_id,
        event.ep_id,
        event.completion_code,
        event.residual,
        event.ptr
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

fn log_xhci_mass_endpoint_state(
    slot_id: u8,
    bulk_out_ep: u8,
    bulk_in_ep: u8,
    reason: &'static str,
) -> Option<u32> {
    let Some(ctrl) = super::discover_first_controller() else {
        crate::log!("crabusb: xhci {} no controller\n", reason);
        return None;
    };

    let mmio = ctrl.mmio_base.as_ptr() as *const u8;
    let cap_len = unsafe { read_volatile(mmio) } as usize;
    let hccparams1 = read_mmio_u32(mmio, 0x10);
    let context_size = if (hccparams1 & (1 << 2)) != 0 {
        64usize
    } else {
        32usize
    };
    let operational = unsafe { mmio.add(cap_len) };
    let dcbaap = read_mmio_u64(operational, 0x30) & !0x3Fu64;
    if dcbaap == 0 {
        crate::log!("crabusb: xhci {} dcbaap=0\n", reason);
        return None;
    }

    let dcbaa_virt = crate::phys::phys_to_virt(dcbaap as usize) as *const u64;
    let slot_ctx_phys = unsafe { read_unaligned(dcbaa_virt.add(slot_id as usize)) };
    if slot_ctx_phys == 0 {
        crate::log!(
            "crabusb: xhci {} slot={} dcbaa entry empty dcbaap=0x{:X}\n",
            reason,
            slot_id,
            dcbaap
        );
        return None;
    }

    let out_ctx = crate::phys::phys_to_virt(slot_ctx_phys as usize) as *const u8;
    crate::log!(
        "crabusb: xhci {} slot={} caplen=0x{:X} hccparams1=0x{:08X} csz={} dcbaap=0x{:X} out_ctx=0x{:X}\n",
        reason,
        slot_id,
        cap_len,
        hccparams1,
        context_size,
        dcbaap,
        slot_ctx_phys
    );
    log_slot_context(out_ctx);

    let bulk_out_dci = endpoint_dci(bulk_out_ep) as usize;
    let bulk_in_dci = endpoint_dci(bulk_in_ep) as usize;
    log_endpoint_context("bulk-out", unsafe {
        out_ctx.add(bulk_out_dci * context_size)
    });
    let bulk_in_ctx = unsafe { out_ctx.add(bulk_in_dci * context_size) };
    log_endpoint_context("bulk-in", bulk_in_ctx);
    Some(unsafe { read_unaligned(bulk_in_ctx as *const u32) } & 0x7)
}

async fn read_and_validate_csw(
    bulk_in: &mut EndpointBulkIn,
    cmd: &'static str,
    expected_tag: u32,
) -> Result<(), MassProbeError> {
    let mut csw = [0u8; 13];
    let mut csw_got = 0usize;
    for _ in 0..BOT_IO_RETRIES {
        csw_got = with_timeout_or_none(bulk_in.submit_and_wait(&mut csw), BOT_IO_TIMEOUT_MS)
            .await
            .ok_or(MassProbeError::Transport("csw-timeout"))?
            .map_err(|_| MassProbeError::Transport("csw-in"))?;
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
            log_transport_debug("cbw-timeout");
            return Err(MassProbeError::Transport("cbw-timeout"));
        };
        sent = result.map_err(|_| {
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
            log_transport_debug("data-timeout");
            return Err(MassProbeError::Transport("data-timeout"));
        };
        got = result.map_err(|_| {
            log_transport_debug("data-in");
            MassProbeError::Transport("data-in")
        })?;
        if got != 0 {
            break;
        }
    }
    read_and_validate_csw(bulk_in, cmd, tag).await?;
    Ok(got)
}

async fn xhci_reset_mass_endpoint(
    device: &mut crab_usb::Device,
    slot_id: u8,
    bulk_out_ep: u8,
    bulk_in_ep: u8,
    ep: u8,
    stage: &'static str,
) {
    crate::log!("crabusb: mass xhci reset stage={} ep=0x{:02X}\n", stage, ep);
    match device.debug_reset_endpoint(ep, false).await {
        Ok(()) => {
            let _ = log_xhci_mass_endpoint_state(
                slot_id,
                bulk_out_ep,
                bulk_in_ep,
                "post-reset-endpoint",
            );
        }
        Err(err) => crate::log!(
            "crabusb: mass xhci reset stage={} ep=0x{:02X} failed: {:?}\n",
            stage,
            ep,
            err
        ),
    }
}

async fn bot_recovery_before_inquiry(
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

pub(crate) async fn probe_mass_bot(
    device: &mut crab_usb::Device,
    bulk_out: &mut EndpointBulkOut,
    bulk_in: &mut EndpointBulkIn,
    interface_number: u8,
    bulk_out_ep: u8,
    bulk_in_ep: u8,
    concept: MassProbeConcept,
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
    if concept.pre_bot_reset {
        crate::log!(
            "crabusb: mass concept={} pre-bot-reset if#{}\n",
            concept.name,
            interface_number
        );
        bot_recovery_before_inquiry(device, interface_number, bulk_out_ep, bulk_in_ep).await?;
    }
    if concept.pre_reset_bulk_out {
        xhci_reset_mass_endpoint(
            device,
            device.slot_id(),
            bulk_out_ep,
            bulk_in_ep,
            bulk_out_ep,
            "preflight-bulk-out",
        )
        .await;
    }
    if concept.pre_reset_bulk_in {
        xhci_reset_mass_endpoint(
            device,
            device.slot_id(),
            bulk_out_ep,
            bulk_in_ep,
            bulk_in_ep,
            "preflight-bulk-in",
        )
        .await;
    }
    let bulk_in_state =
        log_xhci_mass_endpoint_state(device.slot_id(), bulk_out_ep, bulk_in_ep, "pre-inquiry");
    if concept.reset_halted_bulk_in && bulk_in_state == Some(2) {
        crate::log!(
            "crabusb: mass pre-inquiry bulk-in halted; issuing xhci Reset Endpoint ep=0x{:02X}\n",
            bulk_in_ep
        );
        xhci_reset_mass_endpoint(
            device,
            device.slot_id(),
            bulk_out_ep,
            bulk_in_ep,
            bulk_in_ep,
            "pre-inquiry",
        )
        .await;
    }
    let inquiry_read = match bot_command_in(
        bulk_out,
        bulk_in,
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
            if concept.reset_bulk_in_on_data_failure
                && matches!(
                    first_err,
                    MassProbeError::Transport("data-in")
                        | MassProbeError::Transport("data-timeout")
                )
            {
                xhci_reset_mass_endpoint(
                    device,
                    device.slot_id(),
                    bulk_out_ep,
                    bulk_in_ep,
                    bulk_in_ep,
                    "inquiry-data-fail",
                )
                .await;
            }
            if !concept.recover_on_first_failure {
                return Err(first_err);
            }
            crate::log!(
                "crabusb: mass inquiry initial attempt failed: {:?}; applying BOT recovery\n",
                first_err
            );
            bot_recovery_before_inquiry(device, interface_number, bulk_out_ep, bulk_in_ep).await?;
            bot_command_in(
                bulk_out,
                bulk_in,
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

    let mut read_capacity = [0u8; 8];
    let read_capacity_cdb = [0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let read_capacity_read = bot_command_in(
        bulk_out,
        bulk_in,
        "read-capacity10",
        lun,
        &read_capacity_cdb,
        &mut read_capacity,
        0x544F_4E52,
    )
    .await?;
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
    if block_size == 0 {
        return Err(MassProbeError::ShortData {
            cmd: "read-capacity10",
            got: 0,
            need: 1,
        });
    }

    let block_count = u64::from(last_lba) + 1;
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
