//! Intel High Definition Audio ownership and shared PCM playback.

mod playback;
pub(crate) mod vhda;

use trueos_time::Timer;

use crate::r::services::audio_http;

const PCI_OWNER: &str = "intel-hda";
const HDA_SUBCLASS: u8 = 0x03;
const MULTIMEDIA_AUDIO_SUBCLASS: u8 = 0x01;
// LPJB: Tiger Lake-LP Smart Sound can expose the DSP-style 04:01 class.
const TIGER_LAKE_LP_SMART_SOUND: u16 = 0xA0C8;

fn open_playback() -> Option<playback::Playback> {
    let mut claimed = 0usize;
    let mut playback = None;
    for device in crate::pci::find_by_class(crate::pci::class::MULTIMEDIA) {
        let family_class = device.subclass == HDA_SUBCLASS
            || (device.subclass == MULTIMEDIA_AUDIO_SUBCLASS
                && device.device_id == TIGER_LAKE_LP_SMART_SOUND);
        if device.vendor_id != super::INTEL_VENDOR_ID || !family_class {
            continue;
        }
        match crate::pci::claim_device(&device, PCI_OWNER) {
            Ok(()) => {
                claimed += 1;
                crate::log_info!(target: "global";
                    "hda: ownership acquired bdf={:02X}:{:02X}.{} vendor=0x{:04X} device=0x{:04X} class={:02X}:{:02X} owner={} worker_slot={}\n",
                    device.bus, device.slot, device.function, device.vendor_id,
                    device.device_id, device.class, device.subclass,
                    PCI_OWNER, crate::percpu::current_slot()
                );
                if playback.is_none() {
                    match playback::Playback::open(&device) {
                        Ok(output) => {
                            playback = Some(output);
                            crate::log_info!(target: "global";
                                "hda: PCM playback ready bdf={:02X}:{:02X}.{}\n",
                                device.bus, device.slot, device.function
                            );
                        }
                        Err(error) => crate::log_warn!(target: "global";
                            "hda: playback unavailable bdf={:02X}:{:02X}.{}: {}\n",
                            device.bus, device.slot, device.function, error
                        ),
                    }
                }
            }
            Err(error) => crate::log_warn!(target: "global";
                "hda: ownership unavailable bdf={:02X}:{:02X}.{} error={:?}\n",
                device.bus, device.slot, device.function, error
            ),
        }
    }
    if claimed == 0 {
        crate::log_info!(target: "global";
            "hda: no Intel HDA functions claimed worker_slot={}\n",
            crate::percpu::current_slot()
        );
    }
    playback
}

pub(crate) async fn service() {
    let Some(mut playback) = open_playback() else {
        return;
    };
    audio_http::set_local_playback(true);
    vhda::set_available(true);
    let error = loop {
        if let Err(error) = playback.refill(|block| {
            audio_http::render_local(block);
            vhda::mix(block);
        }) {
            break error;
        }
        playback.poll_jack();
        Timer::after_millis(2).await;
    };
    vhda::set_available(false);
    audio_http::set_local_playback(false);
    crate::log_warn!(target: "global"; "hda: playback stopped: {}\n", error);
}
