//! GNA/HDA speech-precondition control plane.
//!
//! This service owns policy and observability for the microphone -> GNA lane.
//! The HDA driver remains the PCM producer and a later GNA driver remains the
//! inference producer. Keeping those roles separate lets this milestone prove
//! PCI/HDA discovery and bounded logging without claiming that capture DMA or
//! neural inference already ran.

use core::sync::atomic::{AtomicU64, Ordering};

use trueos_time::{Duration, Timer};

pub(crate) const POLL_INTERVAL_MS: u64 = 100;
pub(crate) const WAKE_LOG_SOFTCAP_MS: u64 = 250;

const MAX_CONFIDENCE_MILLI: u16 = 1_000;
const INTEL_PCI_VENDOR_ID: u16 = 0x8086;
const GNA3_ALDER_LAKE_DEVICE_ID: u16 = 0x464F;
const GNA3_RAPTOR_LAKE_DEVICE_ID: u16 = 0xA74F;

const _: () = {
    assert!(POLL_INTERVAL_MS != 0);
    assert!(WAKE_LOG_SOFTCAP_MS >= POLL_INTERVAL_MS);
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VoiceActivity {
    Silent,
    Active,
}

impl VoiceActivity {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Silent => "silent",
            Self::Active => "active",
        }
    }

    const fn speech_detected(self) -> u8 {
        match self {
            Self::Silent => 0,
            Self::Active => 1,
        }
    }

    const fn from_payload(payload: u32) -> Self {
        if (payload & 1) == 0 {
            Self::Silent
        } else {
            Self::Active
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FrontendPublishError {
    ConfidenceOutOfRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MailboxSnapshot {
    sequence: u32,
    payload: u32,
}

/// A latest-value mailbox suitable for a future interrupt-side GNA producer.
///
/// One 64-bit atomic publishes the sequence and payload together, so the
/// service cannot observe a sequence from one inference and data from another.
/// Multiple updates between service polls are deliberately coalesced.
struct LatestMailbox {
    word: AtomicU64,
}

impl LatestMailbox {
    const fn new() -> Self {
        Self {
            word: AtomicU64::new(0),
        }
    }

    fn publish(&self, payload: u32) {
        let mut current = self.word.load(Ordering::Relaxed);
        loop {
            let sequence = (current >> 32) as u32;
            let next_sequence = sequence.wrapping_add(1);
            let next = (u64::from(next_sequence) << 32) | u64::from(payload);
            match self.word.compare_exchange_weak(
                current,
                next,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }

    fn snapshot(&self) -> MailboxSnapshot {
        let word = self.word.load(Ordering::Acquire);
        MailboxSnapshot {
            sequence: (word >> 32) as u32,
            payload: word as u32,
        }
    }
}

static VAD_MAILBOX: LatestMailbox = LatestMailbox::new();
static WAKE_MAILBOX: LatestMailbox = LatestMailbox::new();

/// Publish the newest GNA voice-activity result.
///
/// The producer may run more frequently than the service. Only the newest
/// sample is retained, and observable transition logging remains capped by the
/// 100 ms service poll cadence.
#[allow(dead_code)]
pub(crate) fn publish_voice_activity(
    state: VoiceActivity,
    confidence_milli: u16,
) -> Result<(), FrontendPublishError> {
    if confidence_milli > MAX_CONFIDENCE_MILLI {
        return Err(FrontendPublishError::ConfidenceOutOfRange);
    }
    let state_bit = match state {
        VoiceActivity::Silent => 0,
        VoiceActivity::Active => 1,
    };
    let payload = (u32::from(confidence_milli) << 16) | state_bit;
    VAD_MAILBOX.publish(payload);
    Ok(())
}

/// Publish the newest GNA wake-word result.
///
/// `word_id` is model-defined. The logging side emits at most one important
/// wake record per 250 ms and reports how many detections were suppressed.
#[allow(dead_code)]
pub(crate) fn publish_wake_word(
    word_id: u16,
    confidence_milli: u16,
) -> Result<(), FrontendPublishError> {
    if confidence_milli > MAX_CONFIDENCE_MILLI {
        return Err(FrontendPublishError::ConfidenceOutOfRange);
    }
    let payload = (u32::from(confidence_milli) << 16) | u32::from(word_id);
    WAKE_MAILBOX.publish(payload);
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GnaPciSnapshot {
    bus: u8,
    slot: u8,
    function: u8,
    device_id: u16,
    class: u8,
    subclass: u8,
}

const fn is_gna3_device_id(device_id: u16) -> bool {
    matches!(
        device_id,
        GNA3_ALDER_LAKE_DEVICE_ID | GNA3_RAPTOR_LAKE_DEVICE_ID
    )
}

fn discover_gna3_pci() -> Option<GnaPciSnapshot> {
    let mut found = None;
    crate::pci::with_devices(|devices| {
        found = devices.iter().find_map(|device| {
            if device.vendor_id != INTEL_PCI_VENDOR_ID || !is_gna3_device_id(device.device_id) {
                return None;
            }
            Some(GnaPciSnapshot {
                bus: device.bus,
                slot: device.slot,
                function: device.function,
                device_id: device.device_id,
                class: device.class,
                subclass: device.subclass,
            })
        });
    });
    found
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HdaCaptureSnapshot {
    input_streams: u8,
    adc_widgets: usize,
    microphone_pins: usize,
    line_input_pins: usize,
    dma_configured: bool,
}

impl HdaCaptureSnapshot {
    fn state(self) -> &'static str {
        let topology_complete = self.input_streams != 0
            && self.adc_widgets != 0
            && self
                .microphone_pins
                .saturating_add(self.line_input_pins)
                != 0;
        match (topology_complete, self.dma_configured) {
            (true, true) => "capture-ready",
            (true, false) => "discovery-only",
            (false, true) => "capture-invalid",
            (false, false) => "topology-incomplete",
        }
    }
}

fn discover_hda_capture() -> Option<HdaCaptureSnapshot> {
    crate::hda::pcm_capture_capabilities().map(|capabilities| HdaCaptureSnapshot {
        input_streams: capabilities.input_streams,
        adc_widgets: capabilities.adc_widgets,
        microphone_pins: capabilities.microphone_pins,
        line_input_pins: capabilities.line_input_pins,
        dma_configured: capabilities.dma_configured,
    })
}

fn log_gna_endpoint(snapshot: Option<GnaPciSnapshot>) {
    match snapshot {
        Some(device) => crate::log_important!(target: "service";
            "gna-audio-front-end: endpoint=gna3-pci state=present generation=3.0 bdf={:02X}:{:02X}.{} vendor=0x{:04X} device=0x{:04X} class=0x{:02X} subclass=0x{:02X} ownership=probe-only service_mmio=not-configured\n",
            device.bus,
            device.slot,
            device.function,
            INTEL_PCI_VENDOR_ID,
            device.device_id,
            device.class,
            device.subclass,
        ),
        None => crate::log_important!(target: "service";
            "gna-audio-front-end: endpoint=gna3-pci state=missing vendor=0x{:04X} known_device_ids=0x{:04X},0x{:04X}\n",
            INTEL_PCI_VENDOR_ID,
            GNA3_ALDER_LAKE_DEVICE_ID,
            GNA3_RAPTOR_LAKE_DEVICE_ID,
        ),
    }
}

fn log_hda_endpoint(snapshot: Option<HdaCaptureSnapshot>) {
    match snapshot {
        Some(capture) => crate::log_important!(target: "service";
            "gna-audio-front-end: endpoint=hda-capture state={} input_streams={} adc_widgets={} microphone_pins={} line_input_pins={} capture_dma={}\n",
            capture.state(),
            capture.input_streams,
            capture.adc_widgets,
            capture.microphone_pins,
            capture.line_input_pins,
            capture.dma_configured as u8,
        ),
        None => crate::log_important!(target: "service";
            "gna-audio-front-end: endpoint=hda-capture state=unavailable capture_dma=0\n"
        ),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LogSoftCap {
    interval_ms: u64,
    last_admitted_ms: Option<u64>,
}

impl LogSoftCap {
    const fn new(interval_ms: u64) -> Self {
        Self {
            interval_ms,
            last_admitted_ms: None,
        }
    }

    fn admit(&mut self, now_ms: u64) -> bool {
        if self.last_admitted_ms.is_some_and(|last_ms| {
            now_ms.saturating_sub(last_ms) < self.interval_ms
        }) {
            return false;
        }
        self.last_admitted_ms = Some(now_ms);
        true
    }
}

#[inline]
fn monotonic_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1_000) / hz
}

/// System service for the HDA microphone -> GNA precondition lane.
///
/// Until the capture and GNA drivers call the publication functions above,
/// this task reports only hardware discovery. It never fabricates VAD or wake
/// results and it never treats PCI presence as completed inference.
#[trueos_executor::task]
pub(crate) async fn gna_audio_frontend_service_task() {
    crate::log_important!(target: "service";
        "gna-audio-front-end: online contract=hda-microphone>gna3>noise-reduction>vad>wake-word>speech-detected poll_softcap_ms={} wake_log_softcap_ms={} event_policy=transition-or-softcap inference_provider=awaiting-driver\n",
        POLL_INTERVAL_MS,
        WAKE_LOG_SOFTCAP_MS,
    );

    let mut gna_observed = false;
    let mut last_gna = None;
    let mut hda_observed = false;
    let mut last_hda = None;
    let mut last_vad_sequence = 0u32;
    let mut last_wake_sequence = 0u32;
    let mut last_logged_activity = None;
    let mut wake_log_cap = LogSoftCap::new(WAKE_LOG_SOFTCAP_MS);
    let mut wake_suppressed_since_log = 0u64;

    loop {
        let now_ms = monotonic_ms();

        let gna = discover_gna3_pci();
        if !gna_observed || gna != last_gna {
            log_gna_endpoint(gna);
            last_gna = gna;
            gna_observed = true;
        }

        let hda = discover_hda_capture();
        if !hda_observed || hda != last_hda {
            log_hda_endpoint(hda);
            last_hda = hda;
            hda_observed = true;
        }

        let vad = VAD_MAILBOX.snapshot();
        if vad.sequence != last_vad_sequence {
            let updates = u64::from(vad.sequence.wrapping_sub(last_vad_sequence));
            let activity = VoiceActivity::from_payload(vad.payload);
            let confidence_milli = (vad.payload >> 16) as u16;
            if last_logged_activity != Some(activity) {
                crate::log_important!(target: "service";
                    "gna-audio-front-end: signal=voice-activity state={} speech_detected={} confidence_milli={} coalesced_updates={} source=gna-provider\n",
                    activity.as_str(),
                    activity.speech_detected(),
                    confidence_milli,
                    updates.saturating_sub(1),
                );
                last_logged_activity = Some(activity);
            }
            last_vad_sequence = vad.sequence;
        }

        let wake = WAKE_MAILBOX.snapshot();
        if wake.sequence != last_wake_sequence {
            let detections = u64::from(wake.sequence.wrapping_sub(last_wake_sequence));
            let word_id = wake.payload as u16;
            let confidence_milli = (wake.payload >> 16) as u16;
            if wake_log_cap.admit(now_ms) {
                let suppressed = wake_suppressed_since_log
                    .saturating_add(detections.saturating_sub(1));
                crate::log_important!(target: "service";
                    "gna-audio-front-end: signal=wake-word word_id={} confidence_milli={} suppressed_since_previous={} source=gna-provider\n",
                    word_id,
                    confidence_milli,
                    suppressed,
                );
                wake_suppressed_since_log = 0;
            } else {
                wake_suppressed_since_log =
                    wake_suppressed_since_log.saturating_add(detections);
            }
            last_wake_sequence = wake.sequence;
        }

        Timer::after(Duration::from_millis(POLL_INTERVAL_MS)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailbox_publishes_sequence_and_payload_atomically() {
        let mailbox = LatestMailbox::new();
        assert_eq!(
            mailbox.snapshot(),
            MailboxSnapshot {
                sequence: 0,
                payload: 0,
            }
        );
        mailbox.publish(0x1234_5678);
        assert_eq!(
            mailbox.snapshot(),
            MailboxSnapshot {
                sequence: 1,
                payload: 0x1234_5678,
            }
        );
    }

    #[test]
    fn wake_log_softcap_admits_exact_boundary() {
        let mut cap = LogSoftCap::new(WAKE_LOG_SOFTCAP_MS);
        assert!(cap.admit(1_000));
        assert!(!cap.admit(1_249));
        assert!(cap.admit(1_250));
    }

    #[test]
    fn gna3_ids_are_explicit() {
        assert!(is_gna3_device_id(GNA3_ALDER_LAKE_DEVICE_ID));
        assert!(is_gna3_device_id(GNA3_RAPTOR_LAKE_DEVICE_ID));
        assert!(!is_gna3_device_id(0x7E4C));
    }

    #[test]
    fn voice_activity_payload_round_trips() {
        let confidence_milli = 731u16;
        let payload = (u32::from(confidence_milli) << 16) | 1;
        assert_eq!(VoiceActivity::from_payload(payload), VoiceActivity::Active);
        assert_eq!((payload >> 16) as u16, confidence_milli);
    }
}
