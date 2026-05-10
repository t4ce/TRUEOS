use alloc::{string::String, vec::Vec};
use core::{future::Future, task::Poll};
use crab_usb::{err::TransferError, usb_if};
use embassy_time::{Duration as EmbassyDuration, Timer};
use usb_if::host::ControlSetup;

mod bot;
mod uas;

pub(crate) use self::bot::{
    MassTarget, bot_recovery, keepalive_mass_bot, pick_mass_target, probe_mass_bot,
    read_blocks_bot, request_sense_fixed, synchronize_cache_bot, write_blocks_bot,
};
pub(crate) use self::uas::{
    UAS_XHCI_MAX_STREAM_ID, UasCandidate, UasReadStatusKind, UasTarget, UasWriteStatusKind,
    classify_uas_read_status_iu, classify_uas_write_status_iu, exercise_mass_uas_skhynix,
    keepalive_mass_uas_skhynix, pick_skhynix_uas_target, read_blocks_uas_skhynix,
    request_sense_fixed_uas_skhynix_result, send_read10_uas_skhynix, send_write10_uas_skhynix,
    synchronize_cache_uas_skhynix, uas_stream_id_from_tag, write_blocks_uas_skhynix,
};

pub(super) const USB_CLASS_MASS_STORAGE: u8 = 0x08;
pub(super) const USB_SUBCLASS_SCSI: u8 = 0x06;
pub(super) const USB_PROTO_BULK_ONLY: u8 = 0x50;
pub(super) const USB_PROTO_UAS: u8 = 0x62;
pub(super) const BOT_IO_RETRIES: usize = 8;
pub(super) const BOT_IO_TIMEOUT_MS: u64 = crate::allcaps::storage::USB_MASS_BOT_IO_TIMEOUT_MS;
pub(super) const UAS_IO_TIMEOUT_MS: u64 = crate::allcaps::storage::USB_MASS_UAS_IO_TIMEOUT_MS;
pub(super) const UAS_STATUS_GRACE_MS: u64 = 100;
pub(super) const BOT_RECOVERY_SETTLE_MS: u64 =
    crate::allcaps::storage::USB_MASS_BOT_RECOVERY_SETTLE_MS;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MassTransportKind {
    Bot,
    Uas,
}

#[derive(Clone, Debug)]
pub(crate) struct MassTransportPlan {
    pub bot: Option<MassTarget>,
    pub uas: Vec<UasCandidate>,
}

pub(crate) fn inspect_mass_transports(
    configs: &[usb_if::descriptor::ConfigurationDescriptor],
) -> MassTransportPlan {
    MassTransportPlan {
        bot: pick_mass_target(configs),
        uas: uas::collect_uas_candidates(configs),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MassProbeInfo {
    pub block_size: u32,
    pub block_count: u64,
    pub vendor: String,
    pub product: String,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum MassProbeError {
    Transport(&'static str),
    ShortData,
    Csw,
}

impl MassProbeError {
    pub(crate) fn transport_reason(self) -> Option<&'static str> {
        match self {
            MassProbeError::Transport(reason) => Some(reason),
            _ => None,
        }
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
